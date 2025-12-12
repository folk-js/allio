/*! Core types for AXIO.

Regenerate TypeScript types: `npm run typegen`
*/

#![allow(missing_docs)]

mod element;
mod error;
mod event;
mod freshness;
mod geometry;
mod ids;
mod window;

pub use element::Element;
pub use error::{AxioError, AxioResult};
pub use event::{Event, Snapshot, TextRange, TextSelection};
pub use freshness::Recency;
pub use geometry::{Bounds, Point};
pub use ids::{ElementId, ProcessId, WindowId};
pub use window::Window;
