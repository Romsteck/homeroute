use leptos::prelude::*;

#[component]
pub fn Button(
    children: Children,
    #[prop(default = "primary")] variant: &'static str,
    #[prop(default = false)] disabled: bool,
    #[prop(default = false)] loading: bool,
    #[prop(default = "")] class: &'static str,
) -> impl IntoView {
    let variant_class = match variant {
        "secondary" => "bg-gray-600 hover:bg-gray-700 text-white",
        "danger" => "bg-red-600 hover:bg-red-700 text-white",
        "success" => "bg-green-600 hover:bg-green-700 text-white",
        "warning" => "bg-yellow-600 hover:bg-yellow-700 text-white",
        _ => "bg-blue-600 hover:bg-blue-700 text-white",
    };

    view! {
        <button
            disabled=disabled || loading
            class=format!(
                "px-4 py-2 font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2 {} {}",
                variant_class, class
            )
        >
            {loading.then(|| view! {
                <svg class="animate-spin h-4 w-4" viewBox="0 0 24 24">
                    <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" fill="none"/>
                    <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"/>
                </svg>
            })}
            {children()}
        </button>
    }
}
