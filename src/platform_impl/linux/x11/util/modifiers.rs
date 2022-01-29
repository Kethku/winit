use std::{collections::HashMap, slice};

use super::*;

use crate::event::ElementState;
use crate::keyboard::ModifiersState;

// Offsets within XModifierKeymap to each set of keycodes.
// We are only interested in Shift, Control, Alt, and Logo.
//
// There are 8 sets total. The order of keycode sets is:
//     Shift, Lock, Control, Mod1 (Alt), Mod2, Mod3, Mod4 (Logo), Mod5
//
// https://tronche.com/gui/x/xlib/input/XSetModifierMapping.html
const SHIFT_OFFSET: usize = 0;
const CONTROL_OFFSET: usize = 2;
const ALT_OFFSET: usize = 3;
const LOGO_OFFSET: usize = 6;
const NUM_MODS: usize = 8;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Modifier {
    Alt,
    Ctrl,
    Shift,
    Logo,
}

#[derive(Debug, Default)]
pub struct ModifierKeymap {
    // Maps keycodes to modifiers
    keys: HashMap<ffi::KeyCode, Modifier>,
}

#[derive(Clone, Debug, Default)]
pub struct ModifierKeyState {
    // Contains currently pressed modifiers.
    state: ModifiersState,
}

impl ModifierKeymap {
    pub fn new() -> ModifierKeymap {
        ModifierKeymap::default()
    }

    pub fn get_modifier(&self, keycode: ffi::KeyCode) -> Option<Modifier> {
        self.keys.get(&keycode).cloned()
    }

    pub fn reset_from_x_connection(&mut self, xconn: &XConnection) {
        unsafe {
            let keymap = (xconn.xlib.XGetModifierMapping)(xconn.display);

            if keymap.is_null() {
                panic!("failed to allocate XModifierKeymap");
            }

            self.reset_from_x_keymap(&*keymap);

            (xconn.xlib.XFreeModifiermap)(keymap);
        }
    }

    fn reset_from_x_keymap(&mut self, keymap: &ffi::XModifierKeymap) {
        let keys_per_mod = keymap.max_keypermod as usize;

        let keys = unsafe {
            slice::from_raw_parts(keymap.modifiermap as *const _, keys_per_mod * NUM_MODS)
        };

        self.keys.clear();

        self.read_x_keys(keys, SHIFT_OFFSET, keys_per_mod, Modifier::Shift);
        self.read_x_keys(keys, CONTROL_OFFSET, keys_per_mod, Modifier::Ctrl);
        self.read_x_keys(keys, ALT_OFFSET, keys_per_mod, Modifier::Alt);
        self.read_x_keys(keys, LOGO_OFFSET, keys_per_mod, Modifier::Logo);
    }

    fn read_x_keys(
        &mut self,
        keys: &[ffi::KeyCode],
        offset: usize,
        keys_per_mod: usize,
        modifier: Modifier,
    ) {
        let start = offset * keys_per_mod;
        let end = start + keys_per_mod;

        for &keycode in &keys[start..end] {
            if keycode != 0 {
                self.keys.insert(keycode, modifier);
            }
        }
    }
}

impl ModifierKeyState {
    pub fn update_state(
        &mut self,
        state: &ModifiersState,
        except: Option<Modifier>,
    ) -> Option<ModifiersState> {
        let mut new_state = *state;

        match except {
            Some(Modifier::Alt) => new_state.set(ModifiersState::ALT, self.state.alt_key()),
            Some(Modifier::Ctrl) => {
                new_state.set(ModifiersState::CONTROL, self.state.control_key())
            }
            Some(Modifier::Shift) => new_state.set(ModifiersState::SHIFT, self.state.shift_key()),
            Some(Modifier::Logo) => new_state.set(ModifiersState::SUPER, self.state.super_key()),
            None => (),
        }

        if self.state == new_state {
            None
        } else {
            self.state = new_state;
            Some(new_state)
        }
    }

    pub fn modifiers(&self) -> ModifiersState {
        self.state
    }

    pub fn key_event(&mut self, state: ElementState, modifier: Modifier) {
        match state {
            ElementState::Pressed => self.key_press(modifier),
            ElementState::Released => self.key_release(modifier),
        }
    }

    fn key_press(&mut self, modifier: Modifier) {
        set_modifier(&mut self.state, modifier, true);
    }

    fn key_release(&mut self, modifier: Modifier) {
        set_modifier(&mut self.state, modifier, false);
    }
}

fn set_modifier(state: &mut ModifiersState, modifier: Modifier, value: bool) {
    match modifier {
        Modifier::Alt => state.set(ModifiersState::ALT, value),
        Modifier::Ctrl => state.set(ModifiersState::CONTROL, value),
        Modifier::Shift => state.set(ModifiersState::SHIFT, value),
        Modifier::Logo => state.set(ModifiersState::SUPER, value),
    }
}
