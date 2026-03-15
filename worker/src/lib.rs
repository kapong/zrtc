mod handlers;
mod storage;
mod crypto;
mod vacuum;

use worker::*;

#[event(fetch)]
async fn fetch(req: Request, env: Env, ctx: Context) -> Result<Response> {
    let method = req.method();
    let path = req.path();

    // CORS preflight
    if method == Method::Options {
        return handlers::handle_options(&env);
    }

    // Health check
    if method == Method::Get && path == "/" {
        return handlers::handle_health(&env).await;
    }

    // All other routes are POST
    if method != Method::Post {
        return Response::error("Method not allowed", 405);
    }

    match path.as_str() {
        "/new" => handlers::handle_new(req, &env, &ctx, None).await,
        p if p.starts_with("/new/") => {
            let token = p.strip_prefix("/new/").unwrap_or("").to_string();
            if token.is_empty() {
                return Response::error("Token required", 400);
            }
            handlers::handle_new(req, &env, &ctx, Some(token)).await
        }
        "/listen" => handlers::handle_listen(req, &env, &ctx).await,
        "/join" => handlers::handle_join(req, &env, &ctx).await,
        "/poll" => handlers::handle_poll(req, &env, &ctx).await,
        "/hangup" => handlers::handle_hangup(req, &env, &ctx).await,
        _ => Response::error("Not found", 404),
    }
}
