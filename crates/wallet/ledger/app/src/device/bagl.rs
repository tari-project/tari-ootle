//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use borsh::{BorshDeserialize, BorshSerialize};
use ledger_device_sdk::{
    exit_app,
    include_gif,
    io::{Comm, Event, Reply},
    ui::{
        bitmaps::{BACK, CERTIFICATE, DASHBOARD_X, Glyph},
        gadgets::{EventOrPageIndex, MultiPageMenu, Page, popup},
    },
};
use ootle_ledger_common::OotleStatusWord;

use crate::{
    constants::LEDGER_APP_NAME,
    handlers,
    request::{Instruction, Request},
    status::AppStatus,
};

pub fn init(_comm: &mut Comm) {}

pub fn command_fail<E: Into<AppStatus>>(comm: &mut Comm, e: E) {
    match e.into() {
        AppStatus::OotleStatusWord(status) => {
            comm.reply(Reply(status.to_status()));
        },
        AppStatus::StatusWords(status) => {
            comm.reply(status);
        },
        AppStatus::StatusWithMessage { message, status } => {
            popup(&message);
            comm.reply(status);
        },
        AppStatus::OotleStatusWithMessages { messages, status } => {
            for msg in messages {
                popup(&msg);
            }
            comm.reply(Reply(status.to_status()));
        },
    }
}

pub fn next_command(comm: &mut Comm) -> Option<(&mut Comm, Request)> {
    match ui_menu_main(comm) {
        Event::Command(req) => Some((comm, req)),
        Event::Ticker | Event::Button { .. } => {
            // Ignore UI events
            None
        },
    }
}

fn handle<T, R, F>(comm: &mut Comm, handler: F) -> Result<(), AppStatus>
where
    T: BorshDeserialize,
    R: BorshSerialize,
    F: FnOnce(T) -> Result<R, AppStatus>,
{
    let data = comm.get_data().map_err(|_| OotleStatusWord::BadRequest)?;

    let payload = T::try_from_slice(&data).map_err(|_| OotleStatusWord::BadRequest)?;

    let response = handler(payload)?;
    let data = borsh::to_vec(&response).map_err(|_| OotleStatusWord::EncodeResponseFail)?;
    comm.append(&data);
    comm.reply_ok();
    Ok(())
}

pub fn handle_apdu_request((comm, req): (&mut Comm, Request)) {
    let res = match req.instruction {
        Instruction::GetVersion => handle(comm, handlers::get_version),
        Instruction::GetAppName => handle(comm, handlers::get_app_name),
        Instruction::GetPublicKey => handle(comm, handlers::get_public_key),
    };

    match res {
        Ok(_) => {},
        Err(e) => command_fail(comm, e),
    }
}

fn ui_about_menu(comm: &mut Comm) -> Event<Request> {
    pub const COPYRIGHT: &str = "(c) 2026 The Tari Project";
    let pages = [
        &Page::from(([LEDGER_APP_NAME, COPYRIGHT], true)),
        &Page::from(("Back", &BACK)),
    ];
    loop {
        match MultiPageMenu::new(comm, &pages).show() {
            EventOrPageIndex::Event(e) => return e,
            EventOrPageIndex::Index(1) => return ui_menu_main(comm),
            EventOrPageIndex::Index(_) => (),
        }
    }
}

pub fn ui_menu_main(comm: &mut Comm) -> Event<Request> {
    const APP_ICON: Glyph = Glyph::from_include(include_gif!("images/key.gif", BAGL));
    let pages = [
        // The from trait allows to create different styles of pages
        // without having to use the new() function.
        &Page::from((["Tari Ootle", "Wallet"], &APP_ICON)),
        &Page::from((["Version", env!("CARGO_PKG_VERSION")], true)),
        &Page::from(("About", &CERTIFICATE)),
        &Page::from(("Quit", &DASHBOARD_X)),
    ];
    loop {
        match MultiPageMenu::new(comm, &pages).show() {
            EventOrPageIndex::Event(e) => return e,
            EventOrPageIndex::Index(2) => return ui_about_menu(comm),
            EventOrPageIndex::Index(3) => exit_app(0),
            EventOrPageIndex::Index(_) => (),
        }
    }
}

#[allow(dead_code)]
pub fn debug(msg: &str) {
    ledger_device_sdk::ui::gadgets::popup(msg);
}

pub fn show_menu_main(_comm: &mut Comm) {
    // Nothing to do, next_command will call ui_menu_main which will show the menu
}
