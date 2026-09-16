export function buildMcpSetupPrompt(configuration: string): string {
  return `Set up Memivy, my personal memory app, as a local MCP server for this agent on my Mac.

Memivy is already installed. Use its bundled stdio server with the configuration below.

1. Find the MCP configuration method and location supported by this agent. Adapt the JSON to its required format, keeping the command, arguments, and environment values unchanged.
2. Back up the existing configuration, then add or update only the "memivy" entry. Preserve all other servers and settings. If an existing Memivy entry uses a different executable or memory library, ask me which to keep before replacing it.
3. Reload or reconnect the MCP server and verify that this agent can access both "memory_capture" and "memory_search". Do not create memories just to test the connection. Only report success after verifying the tools are available. If a restart, permission, or manual step is needed, tell me exactly what to do. If memory access is disabled, ask me to enable it in Memivy under Settings > External access.
4. Tell me what you changed and the verification result, using the language of our conversation. If you cannot configure MCP in this environment, explain what is missing.

Configuration:
\`\`\`json
${configuration}
\`\`\``;
}
