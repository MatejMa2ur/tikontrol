//! MCP endpoint. Off until the MCP feature lands.

use crate::app::App;
use crate::http::{Req, Resp};
use std::sync::Arc;

pub fn handle(_app: &Arc<App>, _req: &Req) -> Resp {
    Resp::error(404, "MCP is disabled")
}
