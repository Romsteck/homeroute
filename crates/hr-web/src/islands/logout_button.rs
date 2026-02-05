use leptos::prelude::*;

use crate::components::icons::IconLogOut;
use crate::server_fns::auth::Logout;

/// Small logout button (island — hydrated on client).
#[island]
pub fn LogoutButton() -> impl IntoView {
    let logout_action = ServerAction::<Logout>::new();

    view! {
        <ActionForm action=logout_action>
            <button
                type="submit"
                title="Déconnexion"
                class="text-gray-400 hover:text-white transition-colors"
            >
                <IconLogOut class="w-5 h-5"/>
            </button>
        </ActionForm>
    }
}
