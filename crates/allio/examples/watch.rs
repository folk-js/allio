/*!
watch — exercise allio's own watch pipeline on the element under the mouse.

Unlike `axprobe watch` (raw AX), this goes through `Allio`: `element_at` →
`watch` → registry → events. If `axprobe watch` shows `AXValueChanged` firing
but this shows no `element:changed`, the bug is in allio; if both work, the bug
is downstream (allio-ws / client).

`--poll` additionally fetches the value directly every 250ms. Off by default:
a poll refresh updates the cache (and emits) itself, which would mask whether
notifications alone reach clients.

```text
cargo run -p allio --example watch -- [--delay 5] [--secs 30] [--poll]
```
*/

#![allow(
  unsafe_code,
  missing_docs,
  clippy::unwrap_used,
  clippy::expect_used,
  clippy::wildcard_enum_match_arm
)]

use allio::{Allio, Event, Recency};
use objc2_core_foundation::{kCFRunLoopDefaultMode, CFRunLoop};
use objc2_core_graphics::{CGEvent, CGEventSource, CGEventSourceStateID};
use std::time::{Duration, Instant};

fn flag(args: &[String], name: &str, default: u64) -> u64 {
  args
    .iter()
    .position(|a| a == name)
    .and_then(|i| args.get(i + 1))
    .and_then(|s| s.parse().ok())
    .unwrap_or(default)
}

fn mouse() -> (f64, f64) {
  let src = CGEventSource::new(CGEventSourceStateID::CombinedSessionState);
  let ev = CGEvent::new(src.as_deref()).expect("CGEvent");
  let p = CGEvent::location(Some(&ev));
  (p.x, p.y)
}

/// Pump the main run loop (observer callbacks are delivered there).
fn pump(d: Duration) {
  let deadline = Instant::now() + d;
  while Instant::now() < deadline {
    unsafe { CFRunLoop::run_in_mode(kCFRunLoopDefaultMode, 0.05, false) };
  }
}

fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let allio = Allio::new().expect("Allio::new (accessibility permission?)");
  let mut rx = allio.subscribe();

  for i in (1..=flag(&args, "--delay", 5)).rev() {
    eprintln!("  capturing in {i}…");
    pump(Duration::from_secs(1));
  }

  let (x, y) = mouse();
  let el = allio
    .element_at(x, y)
    .expect("element_at")
    .expect("no tracked window under mouse");
  let id = el.id;
  println!(
    "target id={id} role={:?} platform_role={} value={:?} label={:?}",
    el.role, el.platform_role, el.value, el.label
  );
  allio.watch(id).expect("watch");
  eprintln!("  watching — change the value now…");

  // Drain events on a worker thread; the main thread must pump the run loop.
  std::thread::spawn(move || {
    while let Ok(event) = rx.recv_blocking() {
      match event {
        Event::ElementChanged { element } if element.id == id => {
          println!("element:changed value={:?} focused={:?}", element.value, element.focused);
        }
        Event::ElementRemoved { element_id } if element_id == id => {
          println!("element:removed (target)");
        }
        Event::FocusElement { element, previous_element_id } => {
          println!(
            "focus:element -> {} ({}) prev={previous_element_id:?}",
            element.id, element.platform_role
          );
        }
        _ => {}
      }
    }
  });

  let secs = flag(&args, "--secs", 30);
  let deadline = Instant::now() + Duration::from_secs(secs);
  let poll = args.iter().any(|a| a == "--poll");
  let mut last = el.value;
  while Instant::now() < deadline {
    pump(Duration::from_millis(250));
    if !poll {
      continue;
    }
    // Ground truth via a direct fetch, to compare against the event stream.
    if let Ok(now) = allio.get(id, Recency::Current) {
      if now.value != last {
        println!("poll: value now {:?}", now.value);
        last = now.value;
      }
    } else {
      println!("poll: target no longer in registry");
      break;
    }
  }
}
