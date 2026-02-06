use leptos::prelude::*;

/// Read a query parameter from the current SSR request.
pub fn get_query_param(name: &str) -> Option<String> {
    #[cfg(feature = "ssr")]
    {
        use_context::<axum::http::request::Parts>().and_then(|parts| {
            parts.uri.query().and_then(|q| {
                q.split('&')
                    .find_map(|pair| {
                        let (k, v) = pair.split_once('=')?;
                        if k == name {
                            Some(v.replace('+', " "))
                        } else {
                            None
                        }
                    })
            })
        })
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = name;
        None
    }
}

/// SSR flash message component. Reads `?msg=` query param and renders a toast banner.
/// Use `?msg=error&detail=...` for error messages.
#[component]
pub fn FlashMessage() -> impl IntoView {
    let msg = get_query_param("msg");
    let detail = get_query_param("detail");

    msg.map(|m| {
        let is_error = m == "error";
        let text = if is_error {
            detail.unwrap_or_else(|| "Une erreur est survenue".to_string())
        } else {
            m
        };
        let bg = if is_error {
            "bg-red-500/20 border-red-500/50 text-red-300"
        } else {
            "bg-green-500/20 border-green-500/50 text-green-300"
        };

        view! {
            <div
                class=format!("mx-6 mt-4 px-4 py-3 border text-sm flex items-center justify-between {bg}")
            >
                <span>{text}</span>
            </div>
        }
    })
}
