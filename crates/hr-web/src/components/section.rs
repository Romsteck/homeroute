use leptos::prelude::*;

#[component]
pub fn Section(
    #[prop(optional)] title: Option<&'static str>,
    #[prop(default = false)] contrast: bool,
    #[prop(default = "")] class: &'static str,
    children: Children,
) -> impl IntoView {
    let bg = if contrast { "bg-gray-800/50" } else { "bg-gray-900" };
    view! {
        <div class=format!("border-b border-gray-700 {} {}", bg, class)>
            {title.map(|t| view! {
                <div class="px-6 py-3 border-b border-gray-700/50">
                    <h2 class="text-sm font-semibold text-gray-400 uppercase tracking-wider">{t}</h2>
                </div>
            })}
            <div class="px-6 py-3">
                {children()}
            </div>
        </div>
    }
}
