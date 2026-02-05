use leptos::prelude::*;

#[component]
pub fn Card(
    title: &'static str,
    #[prop(optional)] icon: Option<fn() -> AnyView>,
    #[prop(optional)] actions: Option<fn() -> AnyView>,
    #[prop(default = "")] class: &'static str,
    children: Children,
) -> impl IntoView {
    view! {
        <div class=format!("bg-gray-800 border border-gray-700 {}", class)>
            <div class="flex items-center justify-between px-4 py-3 border-b border-gray-700 bg-gray-800/60">
                <h3 class="font-semibold flex items-center gap-2 text-sm">
                    {icon.map(|f| f())}
                    {title}
                </h3>
                {actions.map(|f| view! { <div class="flex gap-2">{f()}</div> })}
            </div>
            <div class="p-4">
                {children()}
            </div>
        </div>
    }
}
