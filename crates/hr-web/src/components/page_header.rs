use leptos::prelude::*;

#[component]
pub fn PageHeader(
    title: &'static str,
    #[prop(optional)] icon: Option<fn() -> AnyView>,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    view! {
        <div class="bg-gray-800 border-b border-gray-700 px-6 py-4">
            <div class="flex items-center justify-between">
                <h1 class="text-xl font-semibold flex items-center gap-3">
                    {icon.map(|f| f())}
                    {title}
                </h1>
                {children.map(|c| view! { <div class="flex items-center gap-2">{c()}</div> })}
            </div>
        </div>
    }
}
