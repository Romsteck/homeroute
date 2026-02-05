use leptos::prelude::*;

#[component]
pub fn StatusBadge(status: &'static str, children: Children) -> impl IntoView {
    let colors = match status {
        "up" => "bg-green-500/20 text-green-400 border-green-500/30",
        "down" => "bg-red-500/20 text-red-400 border-red-500/30",
        "active" => "bg-blue-500/20 text-blue-400 border-blue-500/30",
        _ => "bg-yellow-500/20 text-yellow-400 border-yellow-500/30",
    };

    view! {
        <span class=format!("px-2 py-0.5 text-xs font-medium border {}", colors)>
            {children()}
        </span>
    }
}
