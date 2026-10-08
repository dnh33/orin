# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security-sensitive reports.

Use GitHub's private vulnerability reporting: open the repository's **Security**
tab and click **"Report a vulnerability"**, or go directly to:

https://github.com/dnh33/orin/security/advisories/new

You can expect an acknowledgement within a few days. We will keep you informed
as the report is investigated and a fix is prepared.

## Scope

orin runs entirely on your machine. The daemon listens only on a user-scoped
local socket (Unix) or named pipe (Windows), makes no network requests, and
collects no telemetry. The index stores file names and metadata — never file
contents (until the optional content index ships).

Reports most relevant to this threat model:

- Local privilege boundaries: daemon IPC security, socket/pipe permissions
- Path traversal or command injection through crafted file names
- Crashes or resource exhaustion triggered by filesystem contents
- Memory-safety issues reachable from untrusted input

## Supported versions

Only the latest released version receives security fixes.
