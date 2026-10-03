#![allow(unsafe_code)]

//! Per-vsync drawing via `CAMetalDisplayLink`: the system calls back with a ready drawable, paced
//! to the display the layer is on.

use crate::render::Renderer;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker};
use objc2_foundation::{NSObject, NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{
  CAMetalDisplayLink, CAMetalDisplayLinkDelegate, CAMetalDisplayLinkUpdate, CAMetalLayer,
};

define_class!(
  #[unsafe(super(NSObject))]
  #[name = "AllioShaderTicker"]
  #[ivars = Renderer]
  struct Delegate;

  unsafe impl NSObjectProtocol for Delegate {}

  unsafe impl CAMetalDisplayLinkDelegate for Delegate {
    #[unsafe(method(metalDisplayLink:needsUpdate:))]
    fn needs_update(&self, _link: &CAMetalDisplayLink, update: &CAMetalDisplayLinkUpdate) {
      self.ivars().draw(&update.drawable());
    }
  }
);

/// A running display link. Callbacks arrive on the main run loop, so it must be started and
/// stopped on the main thread.
pub(crate) struct Ticker {
  link: Retained<CAMetalDisplayLink>,
  /// The link only holds its delegate weakly.
  _delegate: Retained<Delegate>,
}

impl Ticker {
  pub(crate) fn start(_main: MainThreadMarker, layer: &CAMetalLayer, renderer: Renderer) -> Self {
    let delegate: Retained<Delegate> = {
      let this = Delegate::alloc().set_ivars(renderer);
      unsafe { msg_send![super(this), init] }
    };
    let link = CAMetalDisplayLink::initWithMetalLayer(CAMetalDisplayLink::alloc(), layer);
    link.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    link.setPreferredFrameLatency(1.0);
    unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
    Self {
      link,
      _delegate: delegate,
    }
  }

  pub(crate) fn stop(&self, _main: MainThreadMarker) {
    self.link.invalidate();
  }
}
