//! orin-mcp — MCP (Model Context Protocol) server for orin.
//!
//! Runs an MCP server over stdio exposing three tools backed by the orin
//! daemon over its local socket / named pipe: `find` (search paths),
//! `stat` (file metadata), and `status` (daemon status). Tool failures are
//! reported as MCP error results, never panics.

mod client;
mod server;

pub use server::OrinServer;
pub use server::tool_definitions;

use rmcp::ServiceExt;

/// Serve MCP requests on stdio until the MCP client disconnects.
///
/// Builds a multi-thread tokio runtime and drives [`serve_stdio`] on it.
/// This is what the CLI's `orin mcp` subcommand calls.
pub fn run_stdio() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(serve_stdio())
}

/// Async form of [`run_stdio`]; must run inside a tokio runtime.
///
/// Connects to the daemon lazily, on each tool call.
pub async fn serve_stdio() -> anyhow::Result<()> {
    let service = OrinServer::new().serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::tool_definitions;
    use rmcp::model::JsonObject;
    use rmcp::model::Tool;
    use serde_json::Value;
    use serde_json::json;

    /// Input schema of one tool, looked up by tool name.
    fn schema_for<'a>(tools: &'a [Tool], name: &str) -> &'a JsonObject {
        let found = tools.iter().find(|tool| tool.name == name);
        let Some(tool) = found else {
            panic!("missing tool {name}");
        };
        tool.input_schema.as_ref()
    }

    /// Sorted argument names declared by a tool's input schema.
    fn arg_names(schema: &JsonObject) -> Vec<String> {
        let properties = schema.get("properties").and_then(Value::as_object);
        let mut names = Vec::new();
        if let Some(properties) = properties {
            names.extend(properties.keys().cloned());
        }
        names.sort_unstable();
        names
    }

    /// Schema object for one argument, when declared.
    fn arg_schema<'a>(schema: &'a JsonObject, arg: &str) -> Option<&'a Value> {
        let properties = schema.get("properties")?;
        let properties = properties.as_object()?;
        properties.get(arg)
    }

    #[test]
    fn tool_definitions_cover_find_stat_status() {
        let tools = tool_definitions();
        let mut names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
        names.sort_unstable();
        assert_eq!(names, ["find", "stat", "status"]);

        let find = schema_for(&tools, "find");
        assert_eq!(arg_names(find), ["limit", "query"]);
        assert_eq!(find.get("required"), Some(&json!(["query"])));
        let query = arg_schema(find, "query").expect("query argument");
        let limit = arg_schema(find, "limit").expect("limit argument");
        assert_eq!(query.get("type"), Some(&json!("string")));
        assert_eq!(limit.get("type"), Some(&json!("integer")));

        let stat = schema_for(&tools, "stat");
        assert_eq!(arg_names(stat), ["path"]);
        assert_eq!(stat.get("required"), Some(&json!(["path"])));
        let path = arg_schema(stat, "path").expect("path argument");
        assert_eq!(path.get("type"), Some(&json!("string")));

        let status = schema_for(&tools, "status");
        assert!(arg_names(status).is_empty());
    }
}
