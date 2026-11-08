//! The Fanimate assistant, without a window: the API as tools for models, an
//! MCP server so other agents can drive Fanimate, adapters for the model
//! providers, and the loop that lets a model work on a project step by step.
//!
//! Nothing here edits a project. Tool calls go to a [`mcp::Backend`] or a
//! [`agent::ToolHost`], which in the app is the editor behind
//! `fanimate_api::rpc`, so everything an assistant does is an API call: one
//! undo step each, checked against the client's permissions.

pub mod agent;
pub mod anthropic;
pub mod describe;
pub mod http;
pub mod link;
pub mod mcp;
pub mod openai;
pub mod prompt;
pub mod provider;
pub mod tools;

pub use mcp::{Backend, ToolError};
