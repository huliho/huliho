// Copyright (C) 2026 Eric Kochen
// SPDX-License-Identifier: AGPL-3.0-only
// Additional terms apply, see NOTICE.

import type { Device } from "@huliho/core";

import { m } from "../../paraglide/messages.js";
import type { Locale } from "../../paraglide/runtime.js";

export function isUnknownDevice(device: Device): boolean {
  return device.browser === null && device.os === null && !device.installed;
}

export function deviceLabel(device: Device, locale: Locale): string {
  const os = device.os ?? m.sessions_unknown_os({}, { locale });
  if (device.installed) {
    return device.phone
      ? m.sessions_device_phone_installed({ os }, { locale })
      : m.sessions_device_installed({ os }, { locale });
  }
  if (isUnknownDevice(device)) {
    return m.sessions_device_unknown({}, { locale });
  }
  const browser = device.browser ?? m.sessions_unknown_browser({}, { locale });
  return device.phone
    ? m.sessions_device_phone({ browser, os }, { locale })
    : m.sessions_device_browser_os({ browser, os }, { locale });
}

// What the toast says once a session is revoked: the phone, else the
// browser or system by name, else nothing about the device.
export function revokedToast(device: Device | undefined, locale: Locale): string {
  if (device?.phone === true) {
    return m.sessions_revoked_phone_toast({}, { locale });
  }
  const name = device?.browser ?? device?.os;
  return name === undefined || name === null
    ? m.sessions_revoked_unknown_toast({}, { locale })
    : m.sessions_revoked_toast({ device: name }, { locale });
}
