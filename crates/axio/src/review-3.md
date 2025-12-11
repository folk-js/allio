I'll do a systematic review of all functions across the three layers. Let me start by reading each layer:

[7 tools called]

Now I have the full picture. Here's a comprehensive review:

---

# Axio Layer Review

## 1. Axio (Public API + Internal Coordination)

### queries.rs

#### Free Functions

| Function                              | Concerns               | Side Effects                                            | Essential?                             |
| ------------------------------------- | ---------------------- | ------------------------------------------------------- | -------------------------------------- |
| `build_entry(handle, window_id, pid)` | Converts Platform→Core | OS calls (fetch_attributes, fetch_parent, element_hash) | ✅ Essential - clean boundary function |

#### Public API (Essential - keep)

| Method                    | Concerns                    | Side Effects                  | Notes                  |
| ------------------------- | --------------------------- | ----------------------------- | ---------------------- |
| `get(id, freshness)`      | Orchestration (cache vs OS) | May trigger `refresh_element` | ✅ Core API            |
| `get_cached(id)`          | Convenience wrapper         | None                          | ✅ Keep as 1-liner     |
| `children(id, freshness)` | Orchestration               | May trigger `fetch_children`  | ✅ Core API            |
| `parent(id, freshness)`   | Orchestration               | May trigger `fetch_parent`    | ✅ Core API            |
| `refresh_element(id)`     | Orchestration               | OS call + registry update     | ✅ Essential           |
| `screen_size()`           | Pass-through                | OS call                       | ✅ Keep                |
| `element_at(x, y)`        | Orchestration               | OS call + registration        | ✅ Core API            |
| `window_root(id)`         | Orchestration               | OS call + registration        | ✅ Core API            |
| `window_focus(id)`        | Orchestration               | OS call + registration        | ✅ Core API            |
| `get_windows()`           | Registry query              | None                          | ✅ Keep                |
| `get_window(id)`          | Registry query              | None                          | ✅ Keep                |
| `get_focused_window()`    | Registry query              | None                          | ✅ Keep                |
| `get_z_order()`           | Registry query              | None                          | ✅ Keep                |
| `get_elements(ids)`       | Registry query              | None                          | ❓ Could be `get` loop |
| `get_all_elements()`      | Registry query              | None                          | ⚠️ Debug only?         |
| `get_snapshot()`          | Registry query              | None                          | ✅ For sync            |
| `get_element()`           | DEPRECATED                  | None                          | 🗑️ Remove              |

My notes:

- We should remove `refresh_element` in favor of `get(id, Freshness::Fresh)`
- lets remove get_cached in favor of `get(id, Freshness::Cached)`
- get_elements appears unnused, lets remove it.
- we can simplify our names so that get_window becomes window, get_windows becomes all_windows, get_all_elements becomes all_elements
- we should definitely remove get_element but be sure that we understand and migrate important behaviour.

#### Internal Methods (Need Review)

| Method                                       | Concerns              | Side Effects                         | Notes                                       |
| -------------------------------------------- | --------------------- | ------------------------------------ | ------------------------------------------- |
| `get_element_by_hash(hash, pid)`             | Registry query        | None                                 | ⚠️ Only used in 2 places                    |
| `find_window_for_handle(handle, pid)`        | Registry query + hash | None                                 | ⚠️ Only for callbacks                       |
| `get_window_at_point(x, y)`                  | Registry query        | None                                 | ✅ Keep                                     |
| `get_window_with_handle(id)`                 | Registry query        | None                                 | ✅ Keep                                     |
| `get_focused_window_for_pid(pid)`            | Registry query        | None                                 | ⚠️ Only for callbacks                       |
| `get_app_handle(pid)`                        | Registry query        | None                                 | ✅ Keep                                     |
| `get_element_handle(id)`                     | Registry query        | None                                 | ⚠️ Extract before OS call pattern           |
| `get_element_for_refresh(id)`                | Registry query        | None                                 | ⚠️ Same as above, slightly different fields |
| `build_and_register(handle, window_id, pid)` | Orchestration         | OS call + registration               | ✅ Essential                                |
| `fetch_children(id, max)`                    | Orchestration         | OS call + registration + tree update | ✅ Internal                                 |
| `fetch_parent(id)`                           | Orchestration         | OS call + registration               | ✅ Internal                                 |
| `fetch_element()`                            | DEPRECATED            |                                      | 🗑️ Remove                                   |

my notes:

- remove fetch_element
- get_element_handle and get_element_for_refresh are near-duplicates, lets remove get_element_for_refresh
- I do not like the `build_and_register` pattern, lets remove it.
- for the other ones only used in a few places or just for callbacks, if there is a clear and obvious refactor lets do it, otherwise report back with proposals.

**Issues Identified:**

1. `get_element_handle` and `get_element_for_refresh` are near-duplicates
2. `find_window_for_handle` and `get_focused_window_for_pid` only exist for callbacks

---

### mutations.rs

#### Public Mutations (Essential)

| Method                 | Concerns      | Side Effects | Notes        |
| ---------------------- | ------------- | ------------ | ------------ |
| `set_value(id, value)` | Orchestration | OS call      | ✅ Essential |
| `perform_click(id)`    | Orchestration | OS call      | ✅ Essential |

#### Sync Operations (Internal - polling)

| Method                                | Concerns          | Side Effects                                               | Notes                     |
| ------------------------------------- | ----------------- | ---------------------------------------------------------- | ------------------------- |
| `sync_windows(windows, skip_removal)` | Orchestration     | OS calls (handles) + registry mutations + process creation | ⚠️ Complex, does too much |
| `sync_focused_window(id)`             | Registry mutation | None                                                       | ✅ Simple                 |
| `sync_mouse(pos)`                     | Registry mutation | None                                                       | ✅ Simple                 |

#### Notification Handlers (Internal - callbacks)

| Method                               | Concerns                       | Side Effects             | Notes                               |
| ------------------------------------ | ------------------------------ | ------------------------ | ----------------------------------- |
| `handle_element_destroyed(id)`       | Registry mutation              | None                     | ✅ Simple                           |
| `handle_focus_changed(pid, element)` | Registry mutation + auto-watch | OS calls (watch/unwatch) | ⚠️ Auto-watch is hidden side effect |
| `handle_selection_changed(...)`      | Registry mutation              | None                     | ✅ Simple                           |
| `handle_element_changed(id, notif)`  | Orchestration                  | OS call (refresh)        | ✅ Simple                           |

#### Process Management

| Method                       | Concerns      | Side Effects                                   | Notes                  |
| ---------------------------- | ------------- | ---------------------------------------------- | ---------------------- |
| `get_or_create_process(pid)` | Orchestration | OS calls (observer, app_handle, notifications) | ⚠️ Complex, race-prone |

my notes: agreed, open to suggestions here!

#### Element Registration

| Method                                     | Concerns          | Side Effects                | Notes              |
| ------------------------------------------ | ----------------- | --------------------------- | ------------------ |
| `register_element(entry)`                  | Orchestration     | OS call (destruction watch) | ✅ Essential       |
| `setup_destruction_watch(id, pid, handle)` | Orchestration     | OS call                     | ✅ Internal helper |
| `update_element_data(id, data)`            | Registry mutation | None                        | ✅ Simple          |
| `set_element_children(id, children)`       | Registry mutation | None                        | ✅ Simple          |

my notes: I am skeptical of the concept of `register` as a pattern.

**Issues Identified:**

1. `sync_windows` is doing way too much - fetching handles, creating processes, updating registry
2. `handle_focus_changed` has hidden auto-watch side effect
3. `get_or_create_process` is complex with race condition handling

---

### subscriptions.rs

| Method        | Concerns      | Side Effects                   | Notes        |
| ------------- | ------------- | ------------------------------ | ------------ |
| `watch(id)`   | Orchestration | OS call (add notifications)    | ✅ Essential |
| `unwatch(id)` | Orchestration | OS call (remove notifications) | ✅ Essential |

---

### mod.rs (Axio struct + PlatformCallbacks)

| Method               | Concerns      | Side Effects   | Notes        |
| -------------------- | ------------- | -------------- | ------------ |
| `new()`              | Construction  | Starts polling | ✅ Essential |
| `with_options(opts)` | Construction  | Starts polling | ✅ Essential |
| `subscribe()`        | Event channel | None           | ✅ Essential |
| `read(f)`            | State access  | None           | ✅ Essential |
| `write(f)`           | State access  | None           | ✅ Essential |

#### PlatformCallbacks Implementation

| Method                          | Concerns      | Side Effects             | Notes                             |
| ------------------------------- | ------------- | ------------------------ | --------------------------------- |
| `on_element_destroyed(id)`      | Delegation    | Registry mutation        | ✅ Thin wrapper                   |
| `on_element_changed(id, notif)` | Delegation    | OS call (refresh)        | ✅ Thin wrapper                   |
| `on_children_changed(id)`       | Delegation    | OS call (fetch_children) | ✅ Thin wrapper                   |
| `on_focus_changed(pid, handle)` | Orchestration | Registration + watch     | ⚠️ Has element registration logic |
| `on_selection_changed(...)`     | Orchestration | Registration             | ⚠️ Has element registration logic |

**Issues Identified:**

1. `on_focus_changed` and `on_selection_changed` do element registration - this is orchestration inside callbacks

---

## 2. Registry (Pure Data + Events)

### Element Operations

| Method                               | Concerns                                 | Side Effects   | Notes                      |
| ------------------------------------ | ---------------------------------------- | -------------- | -------------------------- |
| `get_or_insert_element(entry)`       | Insert/update + tree + orphan resolution | Event emission | ⚠️ Complex - does too much |
| `update_element_data(id, data)`      | Update + change detection                | Event emission | ✅ Clear                   |
| `set_element_children(id, children)` | Tree update                              | Event emission | ✅ Clear                   |
| `remove_element(id)`                 | Cascade removal                          | Event emission | ✅ Clear                   |
| `set_element_watch(id, watch)`       | Store handle                             | None           | ✅ Simple                  |
| `take_element_watch(id)`             | Extract handle                           | None           | ✅ Simple                  |
| `build_element(id)`                  | Derive relationships                     | None           | ✅ Essential               |

### Window Operations

| Method                          | Concerns         | Side Effects   | Notes     |
| ------------------------------- | ---------------- | -------------- | --------- |
| `get_or_insert_window(...)`     | Insert + z-order | Event emission | ✅ Clear  |
| `update_window(id, info)`       | Update + z-order | Event emission | ✅ Clear  |
| `set_window_handle(id, handle)` | Store handle     | None           | ✅ Simple |
| `remove_window(id)`             | Cascade removal  | Event emission | ✅ Clear  |

my notes: our `get_or_insert` semantics have always felt a bit weird, its an odd mix for a registry...

### Process Operations

| Method                          | Concerns         | Side Effects | Notes     |
| ------------------------------- | ---------------- | ------------ | --------- |
| `try_insert_process(id, entry)` | Race-safe insert | None         | ✅ Clear  |
| `has_process(id)`               | Query            | None         | ✅ Simple |
| `get_process(id)`               | Query            | None         | ✅ Simple |

### Focus & Selection

| Method                              | Concerns | Side Effects   | Notes    |
| ----------------------------------- | -------- | -------------- | -------- |
| `set_focused_window(id)`            | Update   | Event emission | ✅ Clear |
| `set_focused_element(pid, element)` | Update   | Event emission | ✅ Clear |
| `set_selection(...)`                | Update   | Event emission | ✅ Clear |
| `set_mouse_position(pos)`           | Update   | Event emission | ✅ Clear |

### Queries (all pure, no issues)

| Method                                    | Notes |
| ----------------------------------------- | ----- |
| `get_element(id)`                         | ✅    |
| `get_element_state(id)`                   | ✅    |
| `find_element_by_hash(hash, pid)`         | ✅    |
| `find_by_hash_in_window(hash, window_id)` | ✅    |
| `get_all_elements()`                      | ✅    |
| `get_window(id)`                          | ✅    |
| `get_window_handle(id)`                   | ✅    |
| `get_all_windows()`                       | ✅    |
| `get_all_window_ids()`                    | ✅    |
| `get_focused_window()`                    | ✅    |
| `get_z_order()`                           | ✅    |
| `get_window_at_point(x, y)`               | ✅    |
| `get_focused_window_for_pid(pid)`         | ✅    |
| `snapshot()`                              | ✅    |

**Issues Identified:**

1. `get_or_insert_element` is overloaded - insert, update, tree linking, orphan resolution all in one

---

## 3. Platform Traits (Pure OS Interface)

### Platform (Static methods)

| Method                              | Concerns    | Notes                    |
| ----------------------------------- | ----------- | ------------------------ |
| `has_permissions()`                 | OS query    | ✅ Pure                  |
| `fetch_windows(exclude_pid)`        | OS query    | ✅ Pure                  |
| `fetch_screen_size()`               | OS query    | ✅ Pure                  |
| `fetch_mouse_position()`            | OS query    | ✅ Pure                  |
| `fetch_window_handle(window)`       | OS query    | ✅ Pure                  |
| `create_observer(pid, callbacks)`   | OS mutation | ⚠️ Takes callbacks trait |
| `start_display_link(callback)`      | OS mutation | ✅ Pure                  |
| `enable_accessibility_for_pid(pid)` | OS mutation | ✅ Pure                  |
| `fetch_focused_element(app_handle)` | OS query    | ✅ Pure                  |
| `app_element(pid)`                  | OS query    | ✅ Pure                  |

### PlatformHandle (Per-element)

| Method                            | Concerns    | Notes   |
| --------------------------------- | ----------- | ------- |
| `fetch_children()`                | OS query    | ✅ Pure |
| `fetch_parent()`                  | OS query    | ✅ Pure |
| `element_hash()`                  | OS query    | ✅ Pure |
| `set_value(value)`                | OS mutation | ✅ Pure |
| `perform_action(action)`          | OS mutation | ✅ Pure |
| `fetch_attributes()`              | OS query    | ✅ Pure |
| `fetch_element_at_position(x, y)` | OS query    | ✅ Pure |
| `fetch_selection()`               | OS query    | ✅ Pure |

### PlatformObserver (Notification management)

| Method                                        | Concerns    | Notes                    |
| --------------------------------------------- | ----------- | ------------------------ |
| `subscribe_app_notifications(pid, callbacks)` | OS mutation | ⚠️ Takes callbacks trait |
| `create_watch(handle, id, notifs, callbacks)` | OS mutation | ⚠️ Takes callbacks trait |

**Issues Identified:**

1. Platform is clean - the `callbacks` parameter is the intended coupling point
2. All methods are pure OS operations

---

# Summary of Issues

## Critical Architectural Issues

1. **`get_or_insert_element` is overloaded** - It does insert OR update, tree linking, orphan resolution, and event emission all in one method. Should be split.

2. **`sync_windows` does too much** - Fetches handles, creates processes, updates registry all in one. Hard to reason about.

3. **Duplicate "extract before OS call" methods** - `get_element_handle` and `get_element_for_refresh` are nearly identical.

4. **Callback methods have registration logic** - `on_focus_changed` and `on_selection_changed` do element registration (which involves OS calls), muddying the callback responsibility.

5. **Auto-watch is a hidden side effect** - `handle_focus_changed` silently watches/unwatches elements.

## Non-Essential Methods (candidates for removal/simplification)

| Method                       | Reason                      |
| ---------------------------- | --------------------------- |
| `get_elements(ids)`          | Can be loop of `get_cached` |
| `get_all_elements()`         | Debug-only?                 |
| `get_element()`              | Already deprecated          |
| `fetch_element()`            | Already deprecated          |
| `find_window_for_handle`     | Only for callbacks          |
| `get_focused_window_for_pid` | Only for callbacks          |

## Recommendations for Cleaner Architecture

1. **Split `get_or_insert_element`** into:

   - `insert(entry)` - always inserts
   - `update(id, data)` - always updates
   - Caller decides which to call based on `find_by_hash_in_window`

2. **Move registration out of callbacks** - `on_focus_changed` should receive an `ElementId` or pre-built `Element`, not a `Handle`

3. **Extract "get for OS call" pattern** into a single method with all needed fields

4. **Make auto-watch explicit** - Either remove it or make it a configuration option

5. **Split `sync_windows`** into:
   - `sync_window_info(windows)` - just update info
   - Handle fetching as separate step
   - Process creation as separate step
