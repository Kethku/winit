use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    sync::Mutex,
};

use lazy_static::lazy_static;

use winapi::{ctypes::c_int, shared::minwindef::HKL, um::winuser};

use crate::{
    keyboard::{Key, KeyCode, ModifiersState, NativeKeyCode},
    platform_impl::platform::keyboard::{native_key_to_code, vkey_to_non_printable, ExScancode},
};

lazy_static! {
    pub static ref LAYOUT_CACHE: Mutex<LayoutCache> = Mutex::new(LayoutCache::default());
}

fn key_pressed(vkey: c_int) -> bool {
    unsafe { (winuser::GetKeyState(vkey) & (1 << 15)) == (1 << 15) }
}

bitflags! {
    pub struct WindowsModifiers : u8 {
        const SHIFT = 1 << 0;
        const CONTROL = 1 << 1;
        const ALT = 1 << 2;
        const CAPS_LOCK = 1 << 3;
        const FLAGS_END = 1 << 4;
    }
}

impl WindowsModifiers {
    pub fn active_modifiers(key_state: &[u8; 256]) -> WindowsModifiers {
        let shift = key_state[winuser::VK_SHIFT as usize] & 0x80 != 0;
        let lshift = key_state[winuser::VK_LSHIFT as usize] & 0x80 != 0;
        let rshift = key_state[winuser::VK_RSHIFT as usize] & 0x80 != 0;

        let control = key_state[winuser::VK_CONTROL as usize] & 0x80 != 0;
        let lcontrol = key_state[winuser::VK_LCONTROL as usize] & 0x80 != 0;
        let rcontrol = key_state[winuser::VK_RCONTROL as usize] & 0x80 != 0;

        let alt = key_state[winuser::VK_MENU as usize] & 0x80 != 0;
        let lalt = key_state[winuser::VK_LMENU as usize] & 0x80 != 0;
        let ralt = key_state[winuser::VK_RMENU as usize] & 0x80 != 0;

        let caps = key_state[winuser::VK_CAPITAL as usize] & 0x01 != 0;

        let mut result = WindowsModifiers::empty();
        if shift || lshift || rshift {
            result.insert(WindowsModifiers::SHIFT);
        }
        if control || lcontrol || rcontrol {
            result.insert(WindowsModifiers::CONTROL);
        }
        if alt || lalt || ralt {
            result.insert(WindowsModifiers::ALT);
        }
        if caps {
            result.insert(WindowsModifiers::CAPS_LOCK);
        }

        println!("Active modifiers: {:?}", result);

        result
    }

    pub fn apply_to_kbd_state(self, key_state: &mut [u8; 256]) {
        if self.intersects(Self::SHIFT) {
            key_state[winuser::VK_SHIFT as usize] |= 0x80;
        } else {
            key_state[winuser::VK_SHIFT as usize] &= !0x80;
            key_state[winuser::VK_LSHIFT as usize] &= !0x80;
            key_state[winuser::VK_RSHIFT as usize] &= !0x80;
        }
        if self.intersects(Self::CONTROL) {
            key_state[winuser::VK_CONTROL as usize] |= 0x80;
        } else {
            key_state[winuser::VK_CONTROL as usize] &= !0x80;
            key_state[winuser::VK_LCONTROL as usize] &= !0x80;
            key_state[winuser::VK_RCONTROL as usize] &= !0x80;
        }
        if self.intersects(Self::ALT) {
            key_state[winuser::VK_MENU as usize] |= 0x80;
        } else {
            key_state[winuser::VK_MENU as usize] &= !0x80;
            key_state[winuser::VK_LMENU as usize] &= !0x80;
            key_state[winuser::VK_RMENU as usize] &= !0x80;
        }
        if self.intersects(Self::CAPS_LOCK) {
            key_state[winuser::VK_CAPITAL as usize] |= 0x80;
        } else {
            key_state[winuser::VK_CAPITAL as usize] &= !0x80;
        }
    }

    /// Removes the control modifier if the alt modifier is not present.
    /// This is useful because on Windows: (Control + Alt) == AltGr
    /// but we don't want to interfere with the AltGr state.
    pub fn remove_only_ctrl(mut self) -> WindowsModifiers {
        if !self.contains(WindowsModifiers::ALT) {
            self.remove(WindowsModifiers::CONTROL);
        }
        self
    }
}

pub struct Layout {
    /// Maps a modifier state to group of key strings
    /// Not using `ModifiersState` here because that object cannot express caps lock
    /// but we need to handle caps lock too.
    ///
    /// This map shouldn't need to exist.
    /// However currently this seems to be the only good way
    /// of getting the label for the pressed key. Note that calling `ToUnicode`
    /// just when the key is pressed/released would be enough if `ToUnicode` wouldn't
    /// change the keyboard state (it clears the dead key). There is a flag to prevent
    /// changing the state but that flag requires Windows 10, version 1607 or newer)
    pub keys: HashMap<WindowsModifiers, HashMap<KeyCode, Key<'static>>>,
    pub has_alt_graph: bool,
}

impl Layout {
    pub fn get_key(
        &self,
        mods: WindowsModifiers,
        scancode: ExScancode,
        keycode: KeyCode,
    ) -> Key<'static> {
        // let ctrl_alt: WindowsModifiers = WindowsModifiers::CONTROL | WindowsModifiers::ALT;
        // if self.has_alt_graph && mods.contains(ctrl_alt) {

        // }

        if let Some(keys) = self.keys.get(&mods) {
            if let Some(key) = keys.get(&keycode) {
                return *key;
            }
        }
        Key::Unidentified(NativeKeyCode::Windows(scancode))
    }
}

#[derive(Default)]
pub struct LayoutCache {
    /// Maps locale identifiers (HKL) to layouts
    pub layouts: HashMap<u64, Layout>,
    pub strings: HashSet<&'static str>,
}

impl LayoutCache {
    /// Checks whether the current layout is already known and
    /// prepares the layout if it isn't known.
    /// The current layout is then returned.
    pub fn get_current_layout<'a>(&'a mut self) -> (u64, &'a Layout) {
        let locale_id = unsafe { winuser::GetKeyboardLayout(0) } as u64;
        match self.layouts.entry(locale_id) {
            Entry::Occupied(entry) => (locale_id, entry.into_mut()),
            Entry::Vacant(entry) => {
                let layout = Self::prepare_layout(&mut self.strings, locale_id);
                (locale_id, entry.insert(layout))
            }
        }
    }

    pub fn get_agnostic_mods(&mut self) -> ModifiersState {
        let (_, layout) = self.get_current_layout();
        let filter_out_altgr = layout.has_alt_graph && key_pressed(winuser::VK_RMENU);
        let mut mods = ModifiersState::empty();
        mods.set(ModifiersState::SHIFT, key_pressed(winuser::VK_SHIFT));
        mods.set(
            ModifiersState::CONTROL,
            key_pressed(winuser::VK_CONTROL) && !filter_out_altgr,
        );
        mods.set(
            ModifiersState::ALT,
            key_pressed(winuser::VK_MENU) && !filter_out_altgr,
        );
        mods.set(
            ModifiersState::META,
            key_pressed(winuser::VK_LWIN) || key_pressed(winuser::VK_RWIN),
        );
        mods
    }

    fn prepare_layout(strings: &mut HashSet<&'static str>, locale_id: u64) -> Layout {
        let mut layout = Layout {
            keys: Default::default(),
            has_alt_graph: false,
        };

        // We initialize the keyboard state with all zeros to
        // simulate a scenario when no modifier is active.
        let mut key_state = [0u8; 256];

        // Iterate through every combination of modifiers
        let mods_end = WindowsModifiers::FLAGS_END.bits;
        for mod_state in 0..mods_end {
            let mut keys_for_this_mod = HashMap::with_capacity(256);

            let mod_state = unsafe { WindowsModifiers::from_bits_unchecked(mod_state) };
            mod_state.apply_to_kbd_state(&mut key_state);

            // Virtual key values are in the domain [0, 255].
            // This is reinforced by the fact that the keyboard state array has 256
            // elements. This array is allowed to be indexed by virtual key values
            // giving the key state for the virtual key used for indexing.
            for vk in 0..256 {
                let scancode = unsafe {
                    winuser::MapVirtualKeyExW(vk, winuser::MAPVK_VK_TO_VSC_EX, locale_id as HKL)
                };
                if scancode == 0 {
                    continue;
                }

                let native_code = NativeKeyCode::Windows(scancode as ExScancode);
                let key_code = native_key_to_code(scancode as ExScancode);
                // Let's try to get the key from just the scancode and vk
                // We don't necessarily know yet if AltGraph is present on this layout so we'll
                // assume it isn't. Then we'll do a second pass where we set the "AltRight" keys to
                // "AltGr" in case we find out that there's an AltGraph.
                let preliminary_key =
                    vkey_to_non_printable(vk as i32, native_code, key_code, locale_id, false);
                match preliminary_key {
                    Key::Unidentified(_) => (),
                    _ => {
                        keys_for_this_mod.insert(key_code, preliminary_key);
                        continue;
                    }
                }

                let unicode = Self::to_unicode_string(&key_state, vk, scancode, locale_id);
                let key = match unicode {
                    ToUnicodeResult::Str(str) => {
                        let static_str = get_or_insert_str(strings, str);
                        Key::Character(static_str)
                    }
                    ToUnicodeResult::Dead(dead_char) => {
                        //println!("{:?} - {:?} produced dead {:?}", key_code, mod_state, dead_char);
                        Key::Dead(dead_char)
                    }
                    ToUnicodeResult::None => {
                        let has_alt = mod_state.contains(WindowsModifiers::ALT);
                        let has_ctrl = mod_state.contains(WindowsModifiers::CONTROL);
                        // HACK: `ToUnicodeEx` seems to fail getting the string for the numpad
                        // divide key, so we handle that explicitly here
                        if !has_alt && !has_ctrl && key_code == KeyCode::NumpadDivide {
                            Key::Character("/")
                        } else {
                            // Just use the unidentified key, we got earlier
                            preliminary_key
                        }
                    }
                };

                // Check for alt graph.
                // The logic is that if a key pressed with no modifier produces
                // a different `Character` from when it's pressed with CTRL+ALT then the layout
                // has AltGr.
                let ctrl_alt: WindowsModifiers = WindowsModifiers::CONTROL | WindowsModifiers::ALT;
                let is_in_ctrl_alt = mod_state == ctrl_alt;
                if !layout.has_alt_graph && is_in_ctrl_alt {
                    // Unwrapping here because if we are in the ctrl+alt modifier state
                    // then the alt modifier state must have come before.
                    let simple_keys = layout.keys.get(&WindowsModifiers::empty()).unwrap();
                    if let Some(Key::Character(key_no_altgr)) = simple_keys.get(&key_code) {
                        if let Key::Character(key) = key {
                            layout.has_alt_graph = key != *key_no_altgr;
                        }
                    }
                }

                keys_for_this_mod.insert(key_code, key);
            }
            layout.keys.insert(mod_state, keys_for_this_mod);
        }

        // Second pass: replace right alt keys with AltGr if the layout has alt graph
        if layout.has_alt_graph {
            for mod_state in 0..mods_end {
                let mod_state = unsafe { WindowsModifiers::from_bits_unchecked(mod_state) };
                if let Some(keys) = layout.keys.get_mut(&mod_state) {
                    if let Some(key) = keys.get_mut(&KeyCode::AltRight) {
                        *key = Key::AltGraph;
                    }
                }
            }
        }

        layout
    }

    fn to_unicode_string(
        key_state: &[u8; 256],
        vkey: u32,
        scancode: u32,
        locale_id: u64,
    ) -> ToUnicodeResult {
        unsafe {
            let mut label_wide = [0u16; 8];
            let mut wide_len = winuser::ToUnicodeEx(
                vkey,
                scancode,
                (&key_state[0]) as *const _,
                (&mut label_wide[0]) as *mut _,
                label_wide.len() as i32,
                0,
                locale_id as HKL,
            );
            if wide_len < 0 {
                // If it's dead, let's run `ToUnicode` again, to consume the dead-key
                wide_len = winuser::ToUnicodeEx(
                    vkey,
                    scancode,
                    (&key_state[0]) as *const _,
                    (&mut label_wide[0]) as *mut _,
                    label_wide.len() as i32,
                    0,
                    locale_id as HKL,
                );
                if wide_len > 0 {
                    let os_string = OsString::from_wide(&label_wide[0..wide_len as usize]);
                    if let Ok(label_str) = os_string.into_string() {
                        if let Some(ch) = label_str.chars().next() {
                            return ToUnicodeResult::Dead(Some(ch));
                        }
                    }
                }
                return ToUnicodeResult::Dead(None);
            }
            if wide_len > 0 {
                let os_string = OsString::from_wide(&label_wide[0..wide_len as usize]);
                if let Ok(label_str) = os_string.into_string() {
                    return ToUnicodeResult::Str(label_str);
                }
            }
        }
        ToUnicodeResult::None
    }
}

pub fn get_or_insert_str(strings: &mut HashSet<&'static str>, string: String) -> &'static str {
    {
        let str_ref = string.as_str();
        if let Some(&existing) = strings.get(str_ref) {
            return existing;
        }
    }
    let leaked = Box::leak(Box::from(string));
    strings.insert(leaked);
    leaked
}

#[derive(Clone, Eq, PartialEq)]
enum ToUnicodeResult {
    Str(String),
    Dead(Option<char>),
    None,
}