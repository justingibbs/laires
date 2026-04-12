# Security Policy

## Reporting a vulnerability

If you discover a security vulnerability in Laires, please report it responsibly by emailing **justingibbs@gmail.com** rather than opening a public issue.

Please include:
- A description of the vulnerability
- Steps to reproduce
- The potential impact

I'll acknowledge receipt within 48 hours and work with you on a fix.

## Security considerations

Laires handles LLM API keys through environment variables (`.env` files). The project is designed so that:

- API keys are never stored in configuration files or source code
- `.env` files are git-ignored by default
- The `is_local()` flag restricts certain operations when using cloud providers
- Custom skills have a permission model (`Enabled`, `Disabled`, `ConditionalOn`)

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |
