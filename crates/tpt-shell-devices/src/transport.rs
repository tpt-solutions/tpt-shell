// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! The transport trait (spec §10.2, M7): a byte stream to one device. Real transports (serial,
//! HID, USB, sockets) implement it per platform. The virtual devices in [`crate::virtual_devices`]
//! implement it too, so code that talks to a device can be tested without one.

use std::fmt;

use crate::id::DeviceId;

/// Why a device operation failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceError {
    /// The device was closed, or its peer went away.
    Closed,
    /// The device cannot do this (for example, reading from a printer).
    NotSupported(&'static str),
    /// The device cannot take more data now. Try again after it has been read.
    Busy,
    /// The open was refused: the device is not in the scope the caller was granted, or it is
    /// not present, or it is already open.
    Denied(String),
    /// The platform failed.
    Io(String),
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceError::Closed => write!(f, "the device is closed"),
            DeviceError::NotSupported(what) => write!(f, "{what} is not supported by this device"),
            DeviceError::Busy => write!(f, "the device cannot take more data yet"),
            DeviceError::Denied(why) => write!(f, "open refused: {why}"),
            DeviceError::Io(why) => write!(f, "device error: {why}"),
        }
    }
}

impl std::error::Error for DeviceError {}

/// A byte stream to one device.
pub trait Transport: Send {
    /// The device's identifier.
    fn id(&self) -> &DeviceId;

    /// Sends `bytes`. Returns [`DeviceError::Busy`] without taking any of them when the device
    /// cannot take them all now.
    fn write(&mut self, bytes: &[u8]) -> Result<(), DeviceError>;

    /// Returns up to `max` bytes the device has sent. An empty result means nothing is waiting.
    fn read(&mut self, max: usize) -> Result<Vec<u8>, DeviceError>;

    /// Runs a USB-style control transfer and returns the data stage. Byte-stream devices do not
    /// have control transfers, so the default refuses.
    fn control(&mut self, request: &ControlRequest) -> Result<Vec<u8>, DeviceError> {
        let _ = request;
        Err(DeviceError::NotSupported("control transfers"))
    }
}

/// A control transfer (USB setup packet).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlRequest {
    /// True for device-to-host (the data stage is read).
    pub direction_in: bool,
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    /// The data stage for host-to-device transfers. Empty for device-to-host.
    pub data: Vec<u8>,
    /// The most bytes the caller accepts for device-to-host transfers.
    pub max_in: u16,
}
