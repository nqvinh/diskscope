//! diskscope: a WizTree-like disk analyzer for Windows with a review-first cleanup flow.
//! Nothing is ever deleted automatically; see `cleanup` for the confirmation gate.

pub mod cleanup;
pub mod cli;
pub mod fmt;
pub mod gui;
pub mod model;
pub mod rules;
pub mod scan;
pub mod sys;
pub mod treemap;
pub mod views;
