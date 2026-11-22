//! Cleanup flow: review list -> explicit confirmation -> execute -> log. Nothing here runs on its
//! own; the GUI is the only caller and the CLI has no access to deletion at all.

pub mod action_log;
pub mod confirm;
pub mod execute;
pub mod export;
pub mod fast_delete;
pub mod history;
pub mod request;
pub mod review;
pub mod tool_actions;
pub mod tool_parse;
pub mod tool_probe;
pub mod tool_runner;
