/*! Core types for AXIO.

Regenerate TypeScript types: `npm run typegen`
*/

#![allow(missing_docs)]

mod element;
mod error;
mod event;
mod geometry;
mod ids;
mod recency;
mod window;

pub use element::Element;
pub use error::{AllioError, AllioResult};
pub use event::{Event, Snapshot, TextRange, TextSelection};
pub use geometry::{Bounds, Point};
pub use ids::{ElementId, ProcessId, WindowId};
pub use recency::Recency;
pub use window::Window;
