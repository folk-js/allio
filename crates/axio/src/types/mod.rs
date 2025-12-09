/*! Core types for AXIO.

Regenerate TypeScript types: `npm run typegen`
*/

#![allow(missing_docs)]

mod element;
mod error;
mod event;
mod geometry;
mod ids;
mod window;

pub use element::AXElement;
pub use error::{AxioError, AxioResult};
pub use event::{Event, Selection, Snapshot, TextRange};
pub use geometry::{Bounds, Point};
pub use ids::{ElementId, ProcessId, WindowId};
pub use window::AXWindow;

