mod grading;
mod rust_regression;
mod tasks;
mod workspace;
pub use grading::{Score, score};
pub use tasks::{Task, catalog, task};
pub use workspace::{PreparedTask, prepare};
