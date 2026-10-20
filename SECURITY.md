# Security policy

## Reporting a vulnerability

Please do not open a public issue for a security problem.

Report it privately:

- Preferred: use GitHub's private vulnerability reporting on this repository (Security tab, "Report a vulnerability").
- Alternative: email **[security contact to be set by TPT Solutions]**.

Include the affected version or commit, a description, steps to reproduce, and the impact as you understand it.

## Response timeline

| Step | Target |
|---|---|
| Acknowledge receipt | within 3 working days |
| Initial assessment and severity | within 7 working days |
| Fix or mitigation plan | within 30 days for high or critical issues |
| Coordinated disclosure | agreed with the reporter, normally after a fix is released |

These targets are commitments for the project, not guarantees for any particular fix.

## Scope

In scope: the tpt-shell crates, the CLI, the supervisor, the templates, and the default configuration.

Out of scope: vulnerabilities in the operating system webview, WebKit, GTK or WebView2 themselves. Report those to their vendors. tpt-shell will still document mitigations where it can.

## Supported versions

No release has been made yet. Once v1.0.0 is tagged, this section will list the supported lines.
