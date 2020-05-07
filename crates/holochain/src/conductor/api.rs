#![deny(missing_docs)]

//! Defines the three Conductor APIs by which other code can communicate
//! with a [Conductor]:
//!
//! - [CellConductorApi], for Cells to communicate with their Conductor
//! - [AppInterfaceApi], for external UIs to e.g. call zome functions on a Conductor
//! - [AdminInterfaceApi], for external processes to e.g. modify ConductorState
//!
//! Each type of API uses a [ConductorHandle] as its exclusive means of conductor access

mod api_cell;
mod api_external;
pub mod error;
mod mock;
pub use api_cell::*;
pub use api_external::*;
pub use mock::MockCellConductorApi;