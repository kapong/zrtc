## ADDED Requirements

### Requirement: `Access-Control-Allow-Origin` is driven by `ALLOWED_ORIGIN` env var
The worker SHALL set the `Access-Control-Allow-Origin` response header to the value of the `ALLOWED_ORIGIN` `[vars]` entry in `wrangler.toml`. When the variable is absent the worker SHALL default to `"*"`.

#### Scenario: Default open CORS when ALLOWED_ORIGIN is not set
- **WHEN** `ALLOWED_ORIGIN` is not defined in wrangler.toml (or empty)
- **THEN** all responses SHALL include `Access-Control-Allow-Origin: *`

#### Scenario: Restricted CORS when ALLOWED_ORIGIN is configured
- **WHEN** `ALLOWED_ORIGIN` is set to `"https://app.example.com"`
- **THEN** all responses SHALL include `Access-Control-Allow-Origin: https://app.example.com` instead of `*`

### Requirement: OPTIONS preflight requests are handled correctly
The worker SHALL respond to `OPTIONS` requests on any path with HTTP 204 and the following headers derived from the same `ALLOWED_ORIGIN` env var:
- `Access-Control-Allow-Origin: <ALLOWED_ORIGIN>`
- `Access-Control-Allow-Methods: GET, POST, OPTIONS`
- `Access-Control-Allow-Headers: Content-Type`
- `Access-Control-Max-Age: 86400`

#### Scenario: Preflight is accepted
- **WHEN** a browser sends an `OPTIONS` request to any worker endpoint
- **THEN** the worker SHALL respond with HTTP 204 and the CORS preflight headers above, with no response body

#### Scenario: Preflight uses configured origin
- **WHEN** `ALLOWED_ORIGIN` is `"https://app.example.com"` and a browser sends `OPTIONS /new`
- **THEN** the response SHALL include `Access-Control-Allow-Origin: https://app.example.com`

### Requirement: `ALLOWED_ORIGIN` is configurable at deploy time
The worker SHALL read the allowed origin from the `ALLOWED_ORIGIN` `[vars]` entry. Operators SHALL be able to override this value per deployment without code changes.

#### Scenario: Default value allows all origins
- **WHEN** `ALLOWED_ORIGIN = "*"` (the default)
- **THEN** all non-preflight responses include `Access-Control-Allow-Origin: *`
