//! Bottom-of-sidebar session telemetry. See `sections` for the compartmentalized
//! widgets and `compose_vitals` layout composer.

pub mod sections;

pub use sections::{VitalsMode, VitalsSection, compose_vitals};
