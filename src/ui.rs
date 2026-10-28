use crate::core::Core;
use anyhow::Result;
use cursive::event::{Event, Key};
use cursive::traits::*;
use cursive::views::{
    Button, Dialog, EditView, LinearLayout, Panel, ResizedView, TextContent, TextView,
};
use cursive::{Cursive, CursiveExt};
use std::sync::{Arc, Mutex};
use tracing::*;

#[derive(Clone, Copy)]
enum Unit {
    Btc,
    Sats,
}

fn convert_amount(amount: f64, from: Unit, to: Unit) -> f64 {
    match (from, to) {
        (Unit::Btc, Unit::Sats) => amount * 100_000_000.0,
        (Unit::Sats, Unit::Btc) => amount / 100_000_000.0,
        _ => amount,
    }
}

/// Initialize and run the user interface.
pub fn run_ui(core: Arc<Core>, balance_content: TextContent) -> Result<()> {
    // initialize Cursive, call setup_siv, run event loop, etc.
    info!("Initializing UI");
    let mut siv = Cursive::default();
    setup_siv(&mut siv, core.clone(), balance_content);
    info!("Starting UI event loop");
    siv.run();
    info!("UI loop ended");
    Ok(())
}

/// Set up the Cursive interface with all necessary components and callbacks.
fn setup_siv(siv: &mut Cursive, core: Arc<Core>, balance_content: TextContent) {
    // Configure Cursive (theme, menubar, layout, callbacks, etc.)
    siv.set_autorefresh(true);
    siv.set_window_title("BRC Wallet".to_string());
    siv.add_global_callback('q', |s| {
        info!("Quit command received");
        s.quit();
    });

    setup_menubar(siv, core.clone());
    setup_layout(siv, core, balance_content);
    siv.add_global_callback(Event::Key(Key::Esc), |siv| siv.select_menubar());
    siv.select_menubar();
}

/// Set up the menu bar with "Send" and "Quit" options.
fn setup_menubar(siv: &mut Cursive, core: Arc<Core>) {
    let core_for_send = core.clone();

    siv.menubar()
        .add_leaf("Send", move |s| {
            show_send_transaction(s, core_for_send.clone());
        })
        .add_leaf("Quit", |s| {
            s.quit();
        });

    siv.set_autohide_menu(false);
}

/// Set up the main layout of the application.
fn setup_layout(siv: &mut Cursive, core: Arc<Core>, balance_content: TextContent) {
    // Instruction line at the top
    let instruction = TextView::new("Press Escape to select the top menu");

    // Balance panel showing live balance
    let balance_panel = Panel::new(TextView::new_with_content(balance_content)).title("Balance");

    // Info layout: keys, contacts, etc.
    let info_layout = create_info_layout(&core);

    // Main vertical layout
    let layout = LinearLayout::vertical()
        .child(instruction)
        .child(balance_panel)
        .child(info_layout);

    // Add it as the main layer
    siv.add_layer(layout);
}

/// Create the information layout containing keys and contacts.
fn create_info_layout(core: &Arc<Core>) -> LinearLayout {
    let mut info_layout = LinearLayout::horizontal();

    // Build keys content (one per line)
    let keys_content = core
        .config
        .my_keys
        .iter()
        .map(|key| format!("{}", key.private.display()))
        .collect::<Vec<String>>()
        .join("\n");

    info_layout.add_child(ResizedView::with_full_width(
        Panel::new(TextView::new(keys_content)).title("Your keys"),
    ));

    // Build contacts content (one per line)
    let contacts_content = core
        .config
        .contacts
        .iter()
        .map(|contact| contact.name.clone())
        .collect::<Vec<String>>()
        .join("\n");

    info_layout.add_child(ResizedView::with_full_width(
        Panel::new(TextView::new(contacts_content)).title("Contacts"),
    ));

    info_layout
}

/// Display the "send transaction" dialog.
fn show_send_transaction(s: &mut Cursive, core: Arc<Core>) {
    info!("Showing send transaction dialog");

    // Shared unit state for the dialog (BTC vs Sats)
    let unit = Arc::new(Mutex::new(Unit::Btc));

    // Clones for the "Send" button closure
    let core_for_send = core.clone();
    let unit_for_send = unit.clone();

    let dialog = Dialog::around(create_transaction_layout(unit))
        .title("Send Transaction")
        .button("Send", move |siv| {
            // Read current unit from the mutex
            let unit_value = {
                let u = unit_for_send.lock().unwrap();
                u.clone() // or *u if Unit: Copy
            };

            send_transaction(siv, core_for_send.clone(), unit_value);
        })
        .button("Cancel", |siv| {
            debug!("Transaction cancelled");
            siv.pop_layer();
        });

    s.add_layer(dialog);
}

/// Create the layout for the transaction dialog.
fn create_transaction_layout(unit: Arc<Mutex<Unit>>) -> LinearLayout {
    LinearLayout::vertical()
        .child(TextView::new("Recipient:"))
        .child(EditView::new().with_name("recipient"))
        .child(TextView::new("Amount:"))
        .child(EditView::new().with_name("amount"))
        .child(create_unit_layout(unit))
}

/// Create the layout for selecting the transaction unit (BTC or Sats).
fn create_unit_layout(unit: Arc<Mutex<Unit>>) -> LinearLayout {
    let unit_for_button = unit.clone();

    LinearLayout::horizontal()
        .child(TextView::new("Unit:"))
        .child(
            TextView::new("BTC") // initial display
                .with_name("unit_display"),
        )
        .child(Button::new("Switch", move |s| {
            switch_unit(s, unit_for_button.clone());
        }))
}

/// Switch the transaction unit between BTC and Sats.
fn switch_unit(s: &mut Cursive, unit: Arc<Mutex<Unit>>) {
    // Flip the unit
    {
        let mut unit_guard = unit.lock().unwrap();
        *unit_guard = match *unit_guard {
            Unit::Btc => Unit::Sats,
            Unit::Sats => Unit::Btc,
        };
    }

    // Read the updated value for display
    let current_unit = {
        let unit_guard = unit.lock().unwrap();
        *unit_guard // assumes Unit: Copy; if not, use unit_guard.clone()
    };

    // Update the text view
    s.call_on_name("unit_display", |view: &mut TextView| {
        view.set_content(match current_unit {
            Unit::Btc => "BTC",
            Unit::Sats => "Sats",
        });
    });
}

/// Process the send transaction request.
fn send_transaction(s: &mut Cursive, core: Arc<Core>, unit: Unit) {
    debug!("Send button pressed");

    // Get recipient from the EditView named "recipient"
    let recipient: String = s
        .call_on_name("recipient", |view: &mut EditView| {
            view.get_content().to_string()
        })
        .unwrap_or_default();

    // Get amount from the EditView named "amount" and parse as f64
    let amount: f64 = s
        .call_on_name("amount", |view: &mut EditView| {
            view.get_content().to_string()
        })
        .and_then(|text| text.parse::<f64>().ok())
        .unwrap_or(0.0);

    // Convert to sats using your existing conversion helper
    let amount_sats = convert_amount(amount, unit, Unit::Sats) as u64;

    info!(
        "Attempting to send transaction to {} for {} satoshis",
        recipient, amount_sats
    );

    // We can't .await inside a Cursive callback, so we:
    // 1) Capture a cb_sink to get back into the UI thread.
    // 2) Spawn a Tokio task that awaits the async call.
    let cb_sink = s.cb_sink().clone();

    tokio::spawn(async move {
        // This returns impl Future<Output = Result<(), anyhow::Error>>
        // so we .await it here.
        let result = core
            .send_transaction_async(recipient.as_str(), amount_sats)
            .await;

        // Send a UI update back to Cursive
        if let Err(e) = cb_sink.send(Box::new(move |siv: &mut Cursive| match result {
            Ok(()) => show_success_dialog(siv),
            Err(err) => show_error_dialog(siv, err),
        })) {
            // If the UI is already shut down, just log it
            eprintln!("Failed to send Cursive callback after transaction: {e}");
        }
    });
}

fn show_success_dialog(s: &mut Cursive) {
    info!("Transaction sent successfully");

    let dialog = Dialog::text("Transaction sent successfully")
        .title("Success")
        .button("OK", |siv| {
            debug!("Closing success dialog");
            // Close the success dialog
            siv.pop_layer();
            // Close the underlying "Send Transaction" dialog
            siv.pop_layer();
        });

    s.add_layer(dialog);
}

/// Display an error dialog when a transaction fails.
fn show_error_dialog(s: &mut Cursive, error: impl std::fmt::Display) {
    error!("Failed to send transaction: {}", error);

    let message = format!("Failed to send transaction: {}", error);

    let dialog = Dialog::text(message).title("Error").button("OK", |siv| {
        debug!("Closing error dialog");
        // Close the error dialog
        siv.pop_layer();
        // Close the underlying "Send Transaction" dialog
        siv.pop_layer();
    });

    s.add_layer(dialog);
}
