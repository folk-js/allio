# AXIO Platform Abstraction Audit

This document audits information loss as data flows from macOS → platform abstraction → registry → events, with decisions on what to implement based on AXIO's goals (software interop and end-user malleability).

Reference: AccessKit (ARIA/Chromium-based) for industry-standard naming.

---

## 1. Current Data Flow

```
macOS AXUIElement
    ↓ (handles.rs get_attributes)
ElementAttributes struct
    ↓ (element.rs build_element_from_handle)
AXElement struct
    ↓ (registry.rs register_element)
Registry storage + Event emission
    ↓ (WebSocket)
TypeScript client
```

---

## 2. Information Loss Points

### 2.1 Role Mapping Gaps

Many macOS roles fall through to `Role::Unknown` silently. When unmapped, only `subrole` preserves the original role string.

**Action:** Add warning log when role falls through to Unknown.

**Missing role mappings to consider:**

| macOS Role    | Priority | Notes                 |
| ------------- | -------- | --------------------- |
| `AXColumn`    | High     | Critical for tables   |
| `AXBrowser`   | Medium   | Finder column browser |
| `AXWebArea`   | Medium   | Web content           |
| `AXGrid`      | Medium   | Grid layouts          |
| `AXColorWell` | Low      | Color picker          |
| `AXDateField` | Low      | Date input            |
| `AXSheet`     | Low      | Modal sheet           |

### 2.2 Element Attributes Not Captured

Currently fetched (10 attributes):

- `AXRole`, `AXSubrole`
- `AXTitle` → `label`
- `AXValue` → `value`
- `AXDescription` → `description`
- `AXPlaceholderValue` → `placeholder`
- `AXPosition` + `AXSize` → `bounds`
- `AXFocused` → `focused`
- `AXEnabled` → `enabled`

**Attributes to add:**

| macOS Attribute                     | AXIO Property               | Priority | Use Case                  |
| ----------------------------------- | --------------------------- | -------- | ------------------------- |
| `AXURL`                             | `url`                       | Critical | File paths in Finder      |
| `AXFilename`                        | (use url)                   | Critical | Filename for documents    |
| `AXSelected`                        | `selected`                  | Critical | Selection state           |
| `AXExpanded`                        | `expanded`                  | Critical | Tree/disclosure state     |
| `AXRowIndex`, `AXColumnIndex`       | `row_index`, `column_index` | Critical | Position in tables        |
| `AXRowCount`, `AXColumnCount`       | `row_count`, `column_count` | Critical | Table dimensions          |
| `AXMinValue`, `AXMaxValue`          | `min_value`, `max_value`    | High     | Range constraints         |
| `AXVisibleRows`, `AXVisibleColumns` | (children alt)              | High     | Virtualized table content |
| `AXHeader`                          | (structural)                | Medium   | Table/outline headers     |

**Note on attribute fetching strategy:** Currently we fetch few enough attributes that it's cheap to always fetch them all (single IPC call via `AXUIElementCopyMultipleAttributeValues`). Adding more attributes to the batch is essentially free. The real cost is IPC per element, not attributes per element.

### 2.3 Table Structure

**Problem:** AXIO only uses `AXChildren`. For virtualized tables, `AXChildren` often returns nothing because non-visible rows aren't real children.

**Table-specific attributes:**

| Attribute          | Returns              | Use Case                        |
| ------------------ | -------------------- | ------------------------------- |
| `AXRows`           | All rows             | Complete data (non-virtualized) |
| `AXVisibleRows`    | Visible rows only    | Virtualized tables              |
| `AXColumns`        | All columns          | Complete structure              |
| `AXVisibleColumns` | Visible columns only | Virtualized tables              |
| `AXChildren`       | **Inconsistent!**    | App-dependent behavior          |

**Decision:** Keep current tree model (children + parents) for now. Changing topology introduces complexity. May revisit when we better understand table interaction patterns. For tables, prefer `AXRows`/`AXVisibleRows` over `AXChildren`.

### 2.4 Value Type Handling

Currently handles:

- `CFString` → `Value::String`
- `CFNumber` → `Value::Integer` or `Value::Float`
- `CFBoolean` → `Value::Boolean`

**Not handled (returns `None`):**

- `CFDate` - Date/time values
- Color values
- `AXValue` wrapped types (CGPoint, CGSize, CFRange - except bounds)

**Decision:**

- Unify Integer/Float into `Value::Number(f64)` - matches JSON semantics, eliminates TypeScript `bigint` pain
- Add Date and Color variants
- Skip CFArray support for now (selection handled via `AXSelected` on individual elements)

### 2.5 Notification Gaps

**Currently supported:**

- `AXUIElementDestroyed`, `AXValueChanged`, `AXTitleChanged`
- `AXFocusedUIElementChanged`, `AXSelectedTextChanged`
- `AXMoved` / `AXResized`, `AXLayoutChanged`

**To add:**

- `AXSelectedChildrenChanged` - Collection selection changes
- `AXSelectedRowsChanged` - Table row selection
- `AXRowExpanded` / `AXRowCollapsed` - Tree expansion

**Not needed:**

- `AXFocusedWindowChanged`, `AXMainWindowChanged` - Handled by polling
- `AXAnnouncementRequested` - Screen reader specific

### 2.6 Actions

**Currently supported:**

- `AXPress`, `AXShowMenu`, `AXIncrement`, `AXDecrement`
- `AXConfirm`, `AXCancel`, `AXRaise`, `AXPick`
- `AXExpand`, `AXCollapse`, `AXScrollToVisible`

**To add:**

- `AXDelete` - Delete item
- `AXSelect` - Select item in collection
- `AXSelectAll` - Select all

---

## 3. Naming Conventions (AccessKit/ARIA Alignment)

Consider renaming for industry alignment:

| Current AXIO            | AccessKit/ARIA            | Decision                                                |
| ----------------------- | ------------------------- | ------------------------------------------------------- |
| `enabled: Option<bool>` | `is_disabled` (flag)      | **Rename to `disabled`** - matches ARIA `aria-disabled` |
| `label`                 | `label`                   | Keep ✓                                                  |
| `description`           | `description`             | Keep ✓                                                  |
| `value`                 | `value`                   | Keep ✓                                                  |
| `focused`               | (tree-level in AccessKit) | Keep as property - makes sense for AXIO's model         |

---

## 4. Proposed AXElement Changes

```rust
pub struct AXElement {
  // === Identity (unchanged) ===
  pub id: ElementId,
  pub window_id: WindowId,
  pub pid: ProcessId,

  // === Tree structure (unchanged) ===
  pub is_root: bool,
  pub parent_id: Option<ElementId>,
  pub children: Option<Vec<ElementId>>,

  // === Role (unchanged) ===
  pub role: Role,
  pub subrole: Option<String>,

  // === Text properties ===
  pub label: Option<String>,
  pub description: Option<String>,
  pub placeholder: Option<String>,
  pub url: Option<String>,              // NEW - file paths, links

  // === Value ===
  pub value: Option<Value>,
  pub min_value: Option<f64>,           // NEW - range min
  pub max_value: Option<f64>,           // NEW - range max
  pub value_step: Option<f64>,          // NEW - slider/stepper increment

  // === Geometry (unchanged) ===
  pub bounds: Option<Bounds>,

  // === States ===
  pub focused: Option<bool>,
  pub disabled: bool,                   // RENAMED from enabled (inverted)
  pub selected: Option<bool>,           // NEW - selection state
  pub expanded: Option<bool>,           // NEW - tree/disclosure state

  // === Table/Collection position ===
  pub row_index: Option<usize>,         // NEW
  pub column_index: Option<usize>,      // NEW
  pub row_count: Option<usize>,         // NEW
  pub column_count: Option<usize>,      // NEW

  // === Actions (unchanged) ===
  pub actions: Vec<Action>,
}
```

---

## 5. Proposed Value Enum Changes

```rust
/// Element value - unified numeric type for JSON/TypeScript compatibility.
///
/// Role provides semantic context:
/// - Stepper → integer (whole number)
/// - Slider → float
/// - TextField → string
/// - Checkbox → boolean
pub enum Value {
  String(String),
  Number(f64),    // Unified: integers as whole f64, floats as-is
  Boolean(bool),
  Date(/* timestamp or chrono */),      // NEW
  Color { r: u8, g: u8, b: u8, a: u8 },  // NEW
}
```

**Rationale for unified Number:**

- JSON only has `number` (no int/float distinction)
- TypeScript: single `number` type, no `bigint` awkwardness
- f64 safely holds integers up to 2^53
- macOS handles coercion on write (passes f64, converts internally for integer controls)
- Role tells you if value should be treated as integer

---

## 6. Proposed Action Additions

```rust
pub enum Action {
  // ... existing ...
  Press,
  ShowMenu,
  Increment,
  Decrement,
  Confirm,
  Cancel,
  Raise,
  Pick,
  Expand,
  Collapse,
  ScrollToVisible,

  // NEW
  Delete,
  Select,
  SelectAll,
}
```

---

## 7. macOS Attribute Mapping Reference

| AXIO Property  | macOS Attribute           |
| -------------- | ------------------------- |
| `url`          | `AXURL`                   |
| `selected`     | `AXSelected`              |
| `expanded`     | `AXExpanded`              |
| `disabled`     | `!AXEnabled`              |
| `row_index`    | `AXRowIndex` or `AXIndex` |
| `column_index` | `AXColumnIndex`           |
| `row_count`    | `AXRowCount`              |
| `column_count` | `AXColumnCount`           |
| `min_value`    | `AXMinValue`              |
| `max_value`    | `AXMaxValue`              |
| `value_step`   | `AXValueIncrement`        |

---

## 8. Implementation Phases

### Phase 1: Critical (Immediate)

1. Add `url: Option<String>` - Finder file paths
2. Add `selected: Option<bool>` - List/table selection
3. Add `expanded: Option<bool>` - Tree nodes, disclosure
4. Add `row_index`, `column_index` - Table cell positions
5. Add `row_count`, `column_count` - Table dimensions
6. Add warning log for unmapped roles
7. Rename `enabled` to `disabled` (invert logic)

### Phase 2: Values & Ranges

8. Add `min_value`, `max_value`, `value_step`
9. Unify `Value::Integer`/`Value::Float` → `Value::Number(f64)`
10. Add `Value::Date` variant
11. Add `Value::Color` variant

### Phase 3: Actions & Notifications

12. Add `Delete`, `Select`, `SelectAll` actions
13. Add selection-related notifications (`AXSelectedChildrenChanged`, etc.)

### Phase 4: Consider Later

14. `toggled: Option<Toggled>` for tri-state checkboxes
15. Relationship properties (`labelled_by`, etc.) - would turn tree into DAG
16. Role-based attribute fetching strategy
17. Alternative table children fetching (`AXVisibleRows`)
18. Multi-select container support (`selected_children`)

---

## 9. Summary

| Area            | Status  | Action                          |
| --------------- | ------- | ------------------------------- |
| File paths      | Missing | Add `url` property              |
| Selection state | Missing | Add `selected` (per-element)    |
| Expansion state | Missing | Add `expanded` property         |
| Table structure | Partial | Add row/column index & counts   |
| Range values    | Missing | Add min/max/step                |
| Value types     | Partial | Unify to Number, add Date/Color |
| Actions         | Partial | Add Delete, Select, SelectAll   |
| Notifications   | Partial | Add selection notifications     |
| Naming          | Mixed   | Rename `enabled` → `disabled`   |

Key principle: **Store structural metadata on nodes.** Tables have `row_count`/`column_count`, cells have `row_index`/`column_index`. Don't infer from tree structure.

---

## 10. Performance Notes

**Current optimization:** Attributes fetched via single IPC call (`AXUIElementCopyMultipleAttributeValues`). Adding more attributes to batch is ~free.

**Real costs:**

- IPC per element (high) - 1000 elements = 1000+ calls
- Attributes per element (low) - batched
- Tree traversal (very high)

**Future optimizations to consider:**

- Lazy attribute fetching (minimal on discovery, full on demand)
- Role-based fetching (sliders get min/max, tables get counts)
- Pagination for large trees
- Static attribute caching (role, min/max rarely change)
