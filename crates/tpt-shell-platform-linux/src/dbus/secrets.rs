// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Keystore on the Secret Service API (`org.freedesktop.secrets`, todo M5 decision 2).
//!
//! Secrets live in the user's default collection (GNOME Keyring, KWallet through its bridge,
//! and similar). Each secret is an item with two attributes, `service` (the app id) and `name`,
//! so one app's secrets cannot be read through another app's id.
//!
//! The session uses the `plain` algorithm. The bus is local and the item is already protected
//! by the keyring's own encryption at rest, so there is no second layer here. Fails closed: if
//! the keyring is locked and unlocking needs a prompt, the call returns an error instead of
//! waiting for a dialog that may never be answered.

use tpt_shell_core::{Error, Result};

use super::bus::{session, Connection, Stream};
use super::wire::Value;

const SERVICE: &str = "org.freedesktop.secrets";
const SERVICE_PATH: &str = "/org/freedesktop/secrets";
const SERVICE_IFACE: &str = "org.freedesktop.Secret.Service";
const COLLECTION_IFACE: &str = "org.freedesktop.Secret.Collection";
const ITEM_IFACE: &str = "org.freedesktop.Secret.Item";
const NO_PATH: &str = "/";

/// Longest secret accepted, in bytes.
pub const MAX_SECRET: usize = 64 * 1024;

/// A keystore for one app id.
pub struct Keystore {
    conn: Connection<Stream>,
    session: String,
    collection: String,
    app_id: String,
}

impl Keystore {
    /// Opens a session and finds the default collection.
    ///
    /// `app_id` scopes the secrets: it is stored as the `service` attribute of every item.
    pub fn open(app_id: &str) -> Result<Keystore> {
        if app_id.is_empty() || app_id.len() > 128 || app_id.contains('\0') {
            return Err(Error::Backend(
                "the keystore app id is empty or too long".into(),
            ));
        }
        let mut conn = session()?;
        let reply = conn.call(
            SERVICE,
            SERVICE_PATH,
            SERVICE_IFACE,
            "OpenSession",
            vec![Value::str("plain"), Value::variant(Value::str(""))],
        )?;
        let session = path_at(&reply, 1)?;
        let reply = conn.call(
            SERVICE,
            SERVICE_PATH,
            SERVICE_IFACE,
            "ReadAlias",
            vec![Value::str("default")],
        )?;
        let collection = path_at(&reply, 0)?;
        if collection == NO_PATH {
            return Err(Error::Backend(
                "there is no default keyring; create one in the desktop's keyring application"
                    .into(),
            ));
        }
        Ok(Keystore {
            conn,
            session,
            collection,
            app_id: app_id.to_string(),
        })
    }

    /// Stores `secret` under `name`, replacing an earlier value with that name.
    pub fn store(&mut self, name: &str, secret: &[u8]) -> Result<()> {
        check_name(name)?;
        if secret.len() > MAX_SECRET {
            return Err(Error::Backend(format!(
                "a secret is limited to {MAX_SECRET} bytes"
            )));
        }
        let collection = self.collection.clone();
        self.unlock(std::slice::from_ref(&collection))?;
        let properties = Value::dict_sv(vec![
            ("org.freedesktop.Secret.Item.Label", Value::str(name)),
            (
                "org.freedesktop.Secret.Item.Attributes",
                self.attributes(name),
            ),
        ]);
        let reply = self.conn.call(
            SERVICE,
            &self.collection,
            COLLECTION_IFACE,
            "CreateItem",
            vec![properties, self.secret_value(secret), Value::Bool(true)],
        )?;
        no_prompt(&reply, 1)?;
        Ok(())
    }

    /// Reads the secret stored under `name`. `None` when there is none.
    pub fn load(&mut self, name: &str) -> Result<Option<Vec<u8>>> {
        check_name(name)?;
        let Some(item) = self.find(name)? else {
            return Ok(None);
        };
        let reply = self.conn.call(
            SERVICE,
            SERVICE_PATH,
            SERVICE_IFACE,
            "GetSecrets",
            vec![
                Value::Array("o".to_string(), vec![Value::object_path(&item)]),
                Value::object_path(&self.session),
            ],
        )?;
        let map = reply
            .first()
            .and_then(Value::as_array)
            .ok_or_else(|| Error::Backend("the keyring gave no secrets map".into()))?;
        for entry in map {
            let Value::DictEntry(_, secret) = entry else {
                continue;
            };
            let fields = secret
                .unwrap_variant()
                .as_struct()
                .ok_or_else(|| Error::Backend("a keyring secret has the wrong shape".into()))?;
            let value = fields
                .get(2)
                .and_then(Value::as_bytes)
                .ok_or_else(|| Error::Backend("a keyring secret has no value".into()))?;
            return Ok(Some(value));
        }
        Ok(None)
    }

    /// Deletes the secret stored under `name`. Returns whether one existed.
    pub fn delete(&mut self, name: &str) -> Result<bool> {
        check_name(name)?;
        let Some(item) = self.find(name)? else {
            return Ok(false);
        };
        let reply = self
            .conn
            .call(SERVICE, &item, ITEM_IFACE, "Delete", vec![])?;
        no_prompt(&reply, 0)?;
        Ok(true)
    }

    fn attributes(&self, name: &str) -> Value {
        Value::Array(
            "{ss}".to_string(),
            vec![
                Value::DictEntry(
                    Box::new(Value::str("service")),
                    Box::new(Value::str(&self.app_id)),
                ),
                Value::DictEntry(Box::new(Value::str("name")), Box::new(Value::str(name))),
            ],
        )
    }

    /// The `(oayays)` secret structure: session, parameters, value, content type.
    fn secret_value(&self, secret: &[u8]) -> Value {
        Value::Struct(vec![
            Value::object_path(&self.session),
            Value::bytes(&[]),
            Value::bytes(secret),
            Value::str("application/octet-stream"),
        ])
    }

    /// Finds the unlocked item for `name`, unlocking the collection first if needed.
    fn find(&mut self, name: &str) -> Result<Option<String>> {
        let attrs = self.attributes(name);
        let reply = self.conn.call(
            SERVICE,
            SERVICE_PATH,
            SERVICE_IFACE,
            "SearchItems",
            vec![attrs],
        )?;
        let locked = paths_at(&reply, 1)?;
        if !locked.is_empty() {
            let collection = self.collection.clone();
            self.unlock(std::slice::from_ref(&collection))?;
            let reply = self.conn.call(
                SERVICE,
                SERVICE_PATH,
                SERVICE_IFACE,
                "SearchItems",
                vec![self.attributes(name)],
            )?;
            return Ok(paths_at(&reply, 0)?.into_iter().next());
        }
        Ok(paths_at(&reply, 0)?.into_iter().next())
    }

    /// Asks the service to unlock `objects`. Refused when that needs a prompt.
    fn unlock(&mut self, objects: &[String]) -> Result<()> {
        let list = Value::Array(
            "o".to_string(),
            objects.iter().map(|o| Value::object_path(o)).collect(),
        );
        let reply = self
            .conn
            .call(SERVICE, SERVICE_PATH, SERVICE_IFACE, "Unlock", vec![list])?;
        no_prompt(&reply, 1)
    }
}

/// Checks a secret's name: 1 to 256 bytes, no NUL.
fn check_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 256 || name.contains('\0') {
        return Err(Error::Backend(
            "a keystore name must be 1-256 bytes with no NUL".into(),
        ));
    }
    Ok(())
}

fn path_at(reply: &[Value], index: usize) -> Result<String> {
    reply
        .get(index)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::Backend("the keyring reply has an unexpected shape".into()))
}

fn paths_at(reply: &[Value], index: usize) -> Result<Vec<String>> {
    let items = reply
        .get(index)
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Backend("the keyring reply has an unexpected shape".into()))?;
    items
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| Error::Backend("the keyring reply holds a non-path".into()))
        })
        .collect()
}

/// Fails when the reply asks for a prompt. A prompt is a dialog the user must answer; the shell
/// does not run one, so the operation is refused rather than left waiting.
fn no_prompt(reply: &[Value], index: usize) -> Result<()> {
    match reply.get(index).and_then(Value::as_str) {
        Some(NO_PATH) | None => Ok(()),
        Some(_) => Err(Error::Backend(
            "the keyring needs a prompt: unlock it in the desktop session, then try again".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_secrets_are_bounded() {
        assert!(check_name("token").is_ok());
        assert!(check_name("").is_err());
        assert!(check_name(&"n".repeat(257)).is_err());
        assert!(check_name("a\0b").is_err());
    }

    #[test]
    fn a_prompt_reply_is_refused() {
        let no_prompt_reply = vec![Value::ObjectPath(NO_PATH.into())];
        assert!(no_prompt(&no_prompt_reply, 0).is_ok());
        let prompt_reply = vec![Value::ObjectPath("/org/prompt/1".into())];
        assert!(no_prompt(&prompt_reply, 0).is_err());
    }

    #[test]
    fn the_keystore_is_unsupported_without_the_feature() {
        #[cfg(not(all(feature = "dbus", unix)))]
        {
            assert!(matches!(
                Keystore::open("demo"),
                Err(Error::Unsupported { .. })
            ));
        }
    }
}
