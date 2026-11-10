# Security Policy

## Supported versions

Only the latest release on the `main` branch receives security fixes.

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes       |
| < 0.1   | No        |

## Reporting a vulnerability

Please **do not** open a public issue for security problems.

Report privately through GitHub's
[private vulnerability reporting](https://github.com/RJCogburn/darkmount-linux-oss/security/advisories/new).

Include:

- What the issue is and where it lives (file, command, udev rule, systemd unit, etc.)
- Steps to reproduce, or a proof of concept
- The impact you expect (e.g. privilege escalation via the udev rule, unsafe firmware handling)

You should get an acknowledgement within 7 days. Once a fix is ready, a
GitHub Security Advisory will be published and you will be credited unless
you ask otherwise.

## Scope

This tool talks to the be quiet! Dark Mount keyboard over USB/HID, installs a
udev rule, and can flash firmware. Issues in those areas are in scope.
Vulnerabilities in the keyboard's own firmware should be reported to
be quiet! directly.
