//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use alloc::{borrow::Cow, vec::Vec};

use borsh::{BorshDeserialize, BorshSerialize};
use ledger_device_sdk::{
    include_gif,
    io::{Comm, CommError, Command, Reply, StatusWords},
    nbgl::{
        Field,
        NbglGlyph,
        init_comm,
        nbgl_home_and_settings::NbglHomeAndSettings,
        nbgl_review::NbglReview,
        nbgl_status::NbglStatus,
    },
};
use ootle_ledger_common::OotleStatusWord;

use crate::{
    constants::LEDGER_APP_NAME,
    handlers::{self, ChunkResult, SignReview},
    request::{Instruction, Request},
    state::State,
    status::AppStatus,
};

pub fn init<const N: usize>(comm: &mut Comm<N>) {
    init_comm(comm);
}

pub fn command_fail<const N: usize, E: Into<AppStatus>>(comm: &mut Comm<N>, e: E) {
    let res = match e.into() {
        AppStatus::OotleStatusWord(status) => comm.begin_response().send(Reply(status.to_status())),
        AppStatus::StatusWords(status) => comm.begin_response().send(status),
        AppStatus::StatusWithMessage { message, status } => {
            NbglStatus::new().text(&message).show(false);
            comm.begin_response().send(status)
        },
        AppStatus::OotleStatusWithMessages { messages, status, .. } => {
            for msg in messages {
                NbglStatus::new().text(&msg).show(false);
            }
            comm.begin_response().send(Reply(status.to_status()))
        },
    };

    if let Err(e) = res {
        // If sending the response fails, there's not much we can do. Panic
        panic!("Failed to send response: {:?}", e);
    }
}

pub fn next_command<const N: usize>(comm: &mut Comm<N>) -> Option<Command<'_, N>> {
    Some(comm.next_command())
}

pub fn handle<T, R, F, const N: usize>(state_mut: &mut State, mut command: Command<N>, handler: F)
where
    T: BorshDeserialize,
    R: BorshSerialize,
    F: FnOnce(&mut State, T) -> Result<R, AppStatus>,
{
    match handle_inner(state_mut, &mut command, handler) {
        Ok(data) => {
            command.reply(&data, StatusWords::Ok).unwrap_or_else(|e| {
                panic!("Failed to send response: {:?}", e);
            });
        },
        Err(e) => {
            command_fail(command.into_comm(), e);
        },
    }
}
fn handle_inner<T, R, F, const N: usize>(
    state_mut: &mut State,
    command: &mut Command<N>,
    handler: F,
) -> Result<Vec<u8>, AppStatus>
where
    T: BorshDeserialize,
    R: BorshSerialize,
    F: FnOnce(&mut State, T) -> Result<R, AppStatus>,
{
    let data = command.get_data();
    let payload = match T::try_from_slice(&data) {
        Ok(p) => p,
        Err(_) => return Err(AppStatus::OotleStatusWord(OotleStatusWord::BadRequest)),
    };

    let response = handler(state_mut, payload)?;
    let data = borsh::to_vec(&response).map_err(|_| OotleStatusWord::EncodeResponseFail)?;
    Ok(data)
}

pub fn handle_apdu_request<const N: usize>(state_mut: &mut State, command: Command<'_, N>) {
    match command.decode::<Request>() {
        Ok(request) => match request.instruction {
            Instruction::GetVersion => handle(state_mut, command, handlers::get_version),
            Instruction::GetAppName => handle(state_mut, command, handlers::get_app_name),
            Instruction::GetPublicKey => handle(state_mut, command, handlers::get_public_key),
            Instruction::SignTransaction => handle_sign(state_mut, command, &request),
        },
        Err(e) => {
            command.into_response().send(e).unwrap();
        },
    }
}

/// Streaming `SignTransaction` handler. Intermediate chunks reply with an empty OK; the final
/// chunk shows the NBGL review and, on approval, replies with the signature.
fn handle_sign<const N: usize>(state_mut: &mut State, mut command: Command<N>, request: &Request) {
    let p1 = request.header.p1;
    let p2 = request.header.p2;

    let outcome = {
        let data = command.get_data();
        handlers::process_chunk(state_mut, p1, p2, &data)
    };

    match outcome {
        Ok(ChunkResult::Ack) => {
            command
                .reply(&[], StatusWords::Ok)
                .unwrap_or_else(|e| panic!("Failed to send response: {:?}", e));
        },
        Ok(ChunkResult::ReadyToSign(review)) => {
            if !confirm_sign(&review) {
                command_fail(
                    command.into_comm(),
                    AppStatus::OotleStatusWord(OotleStatusWord::UserRejected),
                );
                return;
            }
            match build_response(&review) {
                Ok(bytes) => command
                    .reply(&bytes, StatusWords::Ok)
                    .unwrap_or_else(|e| panic!("Failed to send response: {:?}", e)),
                Err(e) => command_fail(command.into_comm(), e),
            }
        },
        Err(e) => command_fail(command.into_comm(), e),
    }
}

fn build_response(review: &SignReview) -> Result<Vec<u8>, AppStatus> {
    let response = handlers::sign_approved(review)?;
    borsh::to_vec(&response).map_err(|_| AppStatus::OotleStatusWord(OotleStatusWord::EncodeResponseFail))
}

/// NBGL tag/value review of the transaction summary, returning whether the user approved.
fn confirm_sign(review: &SignReview) -> bool {
    let rows = handlers::review_fields(review);
    let fields: Vec<Field> = rows
        .iter()
        .map(|(name, value)| Field {
            name: name.as_str(),
            value: value.as_str(),
        })
        .collect();

    NbglReview::new()
        .titles("Review transaction", "", "Sign transaction?")
        .show(&fields)
}

impl From<CommError> for AppStatus {
    fn from(error: CommError) -> Self {
        match error {
            CommError::IoError => AppStatus::StatusWithMessage {
                message: Cow::Borrowed("Communication I/O error"),
                status: StatusWords::Unknown,
            },
            CommError::Overflow => AppStatus::StatusWithMessage {
                message: Cow::Borrowed("Response overflow"),
                status: StatusWords::Unknown,
            },
        }
    }
}

pub fn ui_menu_main<const N: usize>(_: &mut Comm<N>) -> NbglHomeAndSettings {
    // Load glyph from 64x64 4bpp gif file with include_gif macro. Creates an NBGL compatible glyph.
    const TARI: NbglGlyph = NbglGlyph::from_include(include_gif!("images/key_64x64.gif", NBGL));

    const APP_AUTHOR: &str = "The Tari Project";
    // Display the home screen.
    NbglHomeAndSettings::new()
        .glyph(&TARI)
        .infos(LEDGER_APP_NAME, env!("CARGO_PKG_VERSION"), APP_AUTHOR)
}

#[allow(dead_code)]
pub fn debug(msg: &str) {
    NbglStatus::new().text(msg).show(false);
}

pub fn show_menu_main(comm: &mut Comm) {
    ui_menu_main(comm).show_and_return()
}
