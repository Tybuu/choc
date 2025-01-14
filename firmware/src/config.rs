use embassy_time::Duration;

use crate::{codes::KeyCodes, keys::Keys};
const SCROLL_TIME: u64 = 500;
const MOUSE_POINTER_TIME: u64 = 5;

pub fn load_callum<const S: usize>(keys: &mut Keys<S>) {
    *keys = Keys::<S>::default();
    // Layer 0
    keys.set_code(KeyCodes::KeyboardQq, 0, 0);
    keys.set_code(KeyCodes::KeyboardWw, 1, 0);
    keys.set_code(KeyCodes::KeyboardEe, 2, 0);
    keys.set_code(KeyCodes::KeyboardRr, 3, 0);
    keys.set_code(KeyCodes::KeyboardTt, 4, 0);

    keys.set_code(KeyCodes::KeyboardAa, 5, 0);
    keys.set_code(KeyCodes::KeyboardSs, 6, 0);
    keys.set_code(KeyCodes::KeyboardDd, 7, 0);
    keys.set_code(KeyCodes::KeyboardFf, 8, 0);
    keys.set_code(KeyCodes::KeyboardGg, 9, 0);

    keys.set_code(KeyCodes::KeyboardZz, 10, 0);
    keys.set_code(KeyCodes::KeyboardXx, 11, 0);
    keys.set_code(KeyCodes::KeyboardCc, 12, 0);
    keys.set_code(KeyCodes::KeyboardVv, 13, 0);
    keys.set_code(KeyCodes::KeyboardBb, 14, 0);

    keys.set_combined(KeyCodes::Layer1, KeyCodes::Layer3, 34, 16, 0);
    keys.set_code(KeyCodes::KeyboardSpacebar, 17, 0);

    keys.set_code(KeyCodes::KeyboardYy, 18, 0);
    keys.set_code(KeyCodes::KeyboardUu, 19, 0);
    keys.set_code(KeyCodes::KeyboardIi, 20, 0);
    keys.set_code(KeyCodes::KeyboardOo, 21, 0);
    keys.set_code(KeyCodes::KeyboardPp, 22, 0);

    keys.set_code(KeyCodes::KeyboardHh, 23, 0);
    keys.set_code(KeyCodes::KeyboardJj, 24, 0);
    keys.set_code(KeyCodes::KeyboardKk, 25, 0);
    keys.set_code(KeyCodes::KeyboardLl, 26, 0);
    keys.set_code(KeyCodes::KeyboardSemiColon, 27, 0);

    keys.set_code(KeyCodes::KeyboardNn, 28, 0);
    keys.set_code(KeyCodes::KeyboardMm, 29, 0);
    keys.set_code(KeyCodes::KeyboardCommaLess, 30, 0);
    keys.set_code(KeyCodes::KeyboardPeriodGreater, 31, 0);
    keys.set_code(KeyCodes::KeyboardSlashQuestion, 32, 0);

    keys.set_code(KeyCodes::KeyboardLeftShift, 33, 0);
    keys.set_combined(KeyCodes::Layer2, KeyCodes::Layer3, 16, 34, 0);
    keys.set_code(KeyCodes::KeyboardRightControl, 35, 0);

    // Layer 1
    keys.set_code(KeyCodes::KeyboardTab, 0, 1);
    // keys.set_code(KeyCodes::KeyboardWw, 2, 1);
    // keys.set_code(KeyCodes::KeyboardEe, 3, 1);
    // keys.set_code(KeyCodes::KeyboardRr, 4, 1);
    keys.set_code(KeyCodes::KeyboardVolumeUp, 4, 1);

    keys.set_code(KeyCodes::KeyboardLeftShift, 5, 1);
    keys.set_code(KeyCodes::KeyboardLeftControl, 6, 1);
    keys.set_code(KeyCodes::KeyboardLeftAlt, 7, 1);
    keys.set_code(KeyCodes::KeyboardLeftGUI, 8, 1);
    keys.set_code(KeyCodes::KeyboardVolumeDown, 9, 1);

    let func = |x: u64| -> u64 { ((10000 * x.pow(2)) / (x.pow(2) + 50000)) + 1000 };
    keys.set_interval(
        KeyCodes::MouseScrollDown,
        Duration::from_millis(SCROLL_TIME),
        func,
        10,
        1,
    );
    keys.set_interval(
        KeyCodes::MouseScrollUp,
        Duration::from_millis(SCROLL_TIME),
        func,
        11,
        1,
    );
    keys.set_code(KeyCodes::MouseLeftClick, 12, 1);
    keys.set_code(KeyCodes::MouseMiddleClick, 13, 1);
    keys.set_code(KeyCodes::MouseRightClick, 14, 1);

    keys.set_combined(KeyCodes::Layer1, KeyCodes::Layer3, 34, 16, 1);
    keys.set_code(KeyCodes::KeyboardSpacebar, 17, 1);

    keys.set_code(KeyCodes::KeyboardCapsLock, 18, 1);
    // keys.set_code(KeyCodes::KeyboardUu, 22, 1);
    // keys.set_code(KeyCodes::KeyboardIi, 23, 1);
    // keys.set_code(KeyCodes::KeyboardOo, 24, 1);
    keys.set_code(KeyCodes::KeyboardDelete, 22, 1);

    keys.set_code(KeyCodes::KeyboardLeftArrow, 23, 1);
    keys.set_code(KeyCodes::KeyboardDownArrow, 24, 1);
    keys.set_code(KeyCodes::KeyboardUpArrow, 25, 1);
    keys.set_code(KeyCodes::KeyboardRightArrow, 26, 1);
    keys.set_code(KeyCodes::KeyboardBackspace, 27, 1);

    // keys.set_code(KeyCodes::KeyboardNn, 33, 1);
    // keys.set_toggle_layer(KeyCodes::Layer4, 33, 1);
    keys.set_interval(
        KeyCodes::MouseNegativeX,
        Duration::from_millis(MOUSE_POINTER_TIME),
        func,
        28,
        1,
    );
    keys.set_interval(
        KeyCodes::MousePositiveY,
        Duration::from_millis(MOUSE_POINTER_TIME),
        func,
        29,
        1,
    );
    keys.set_interval(
        KeyCodes::MouseNegativeY,
        Duration::from_millis(MOUSE_POINTER_TIME),
        func,
        30,
        1,
    );
    keys.set_interval(
        KeyCodes::MousePositiveX,
        Duration::from_millis(MOUSE_POINTER_TIME),
        func,
        31,
        1,
    );
    keys.set_code(KeyCodes::KeyboardEnter, 32, 1);

    keys.set_code(KeyCodes::KeyboardLeftShift, 33, 1);
    keys.set_combined(KeyCodes::Layer2, KeyCodes::Layer3, 16, 34, 1);

    // Layer 2
    keys.set_code(KeyCodes::KeyboardEscape, 0, 2);
    keys.set_code(KeyCodes::KeyboardOpenBracketBrace, 1, 2);
    keys.set_double(
        KeyCodes::KeyboardOpenBracketBrace,
        KeyCodes::KeyboardLeftShift,
        2,
        2,
    );
    keys.set_double(
        KeyCodes::Keyboard9OpenParens,
        KeyCodes::KeyboardLeftShift,
        3,
        2,
    );
    keys.set_double(
        KeyCodes::KeyboardBacktickTilde,
        KeyCodes::KeyboardLeftShift,
        4,
        2,
    );

    keys.set_code(KeyCodes::KeyboardDashUnderscore, 5, 2);
    keys.set_double(
        KeyCodes::Keyboard8Asterisk,
        KeyCodes::KeyboardLeftShift,
        6,
        2,
    );
    keys.set_code(KeyCodes::KeyboardEqualPlus, 7, 2);
    keys.set_double(
        KeyCodes::KeyboardDashUnderscore,
        KeyCodes::KeyboardLeftShift,
        8,
        2,
    );
    keys.set_double(KeyCodes::Keyboard4Dollar, KeyCodes::KeyboardLeftShift, 9, 2);

    keys.set_double(
        KeyCodes::KeyboardEqualPlus,
        KeyCodes::KeyboardLeftShift,
        10,
        2,
    );
    keys.set_double(
        KeyCodes::KeyboardBackslashBar,
        KeyCodes::KeyboardLeftShift,
        11,
        2,
    );
    keys.set_double(KeyCodes::Keyboard2At, KeyCodes::KeyboardLeftShift, 12, 2);
    keys.set_code(KeyCodes::KeyboardSingleDoubleQuote, 13, 2);
    keys.set_double(
        KeyCodes::Keyboard5Percent,
        KeyCodes::KeyboardLeftShift,
        14,
        2,
    );

    keys.set_combined(KeyCodes::Layer1, KeyCodes::Layer3, 34, 16, 2);
    keys.set_code(KeyCodes::KeyboardSpacebar, 17, 2);

    keys.set_double(KeyCodes::Keyboard6Caret, KeyCodes::KeyboardLeftShift, 18, 2);
    keys.set_double(
        KeyCodes::Keyboard0CloseParens,
        KeyCodes::KeyboardLeftShift,
        19,
        2,
    );
    keys.set_double(
        KeyCodes::KeyboardCloseBracketBrace,
        KeyCodes::KeyboardLeftShift,
        20,
        2,
    );
    keys.set_code(KeyCodes::KeyboardCloseBracketBrace, 21, 2);
    keys.set_code(KeyCodes::KeyboardBacktickTilde, 22, 2);

    keys.set_double(KeyCodes::Keyboard3Hash, KeyCodes::KeyboardLeftShift, 23, 2);
    keys.set_code(KeyCodes::KeyboardRightGUI, 24, 2);
    keys.set_code(KeyCodes::KeyboardRightAlt, 25, 2);
    keys.set_code(KeyCodes::KeyboardRightControl, 26, 2);
    keys.set_code(KeyCodes::KeyboardRightShift, 27, 2);

    // keys.set_code(KeyCodes::KeyboardBackslashBar, 33, 2);
    keys.set_code(KeyCodes::KeyboardBackslashBar, 29, 2);
    keys.set_double(
        KeyCodes::Keyboard7Ampersand,
        KeyCodes::KeyboardLeftShift,
        30,
        2,
    );
    keys.set_double(
        KeyCodes::KeyboardSingleDoubleQuote,
        KeyCodes::KeyboardLeftShift,
        31,
        2,
    );
    keys.set_double(
        KeyCodes::Keyboard1Exclamation,
        KeyCodes::KeyboardLeftShift,
        32,
        2,
    );

    keys.set_code(KeyCodes::KeyboardLeftShift, 33, 2);
    keys.set_combined(KeyCodes::Layer2, KeyCodes::Layer3, 16, 34, 2);

    // Layer 3
    keys.set_code(KeyCodes::Keyboard1Exclamation, 0, 3);
    keys.set_code(KeyCodes::Keyboard2At, 1, 3);
    keys.set_code(KeyCodes::Keyboard3Hash, 2, 3);
    keys.set_code(KeyCodes::Keyboard4Dollar, 3, 3);
    keys.set_code(KeyCodes::Keyboard5Percent, 4, 3);

    keys.set_code(KeyCodes::KeyboardLeftShift, 5, 3);
    keys.set_code(KeyCodes::KeyboardLeftControl, 6, 3);
    keys.set_code(KeyCodes::KeyboardLeftAlt, 7, 3);
    keys.set_code(KeyCodes::KeyboardLeftGUI, 8, 3);
    keys.set_code(KeyCodes::KeyboardF11, 9, 3);

    keys.set_code(KeyCodes::KeyboardF1, 10, 3);
    keys.set_code(KeyCodes::KeyboardF2, 11, 3);
    keys.set_code(KeyCodes::KeyboardF3, 12, 3);
    keys.set_code(KeyCodes::KeyboardF4, 13, 3);
    keys.set_code(KeyCodes::KeyboardF5, 14, 3);

    keys.set_combined(KeyCodes::Layer1, KeyCodes::Layer3, 34, 16, 3);
    keys.set_code(KeyCodes::KeyboardSpacebar, 17, 3);

    keys.set_code(KeyCodes::Keyboard6Caret, 18, 3);
    keys.set_code(KeyCodes::Keyboard7Ampersand, 19, 3);
    keys.set_code(KeyCodes::Keyboard8Asterisk, 20, 3);
    keys.set_code(KeyCodes::Keyboard9OpenParens, 21, 3);
    keys.set_code(KeyCodes::Keyboard0CloseParens, 22, 3);

    keys.set_code(KeyCodes::KeyboardF12, 23, 3);
    keys.set_code(KeyCodes::KeyboardRightGUI, 24, 3);
    keys.set_code(KeyCodes::KeyboardRightAlt, 25, 3);
    keys.set_code(KeyCodes::KeyboardRightControl, 26, 3);
    keys.set_code(KeyCodes::KeyboardRightShift, 27, 3);

    keys.set_code(KeyCodes::KeyboardF6, 28, 3);
    keys.set_code(KeyCodes::KeyboardF7, 29, 3);
    keys.set_code(KeyCodes::KeyboardF8, 30, 3);
    keys.set_code(KeyCodes::KeyboardF9, 31, 3);
    keys.set_code(KeyCodes::KeyboardF10, 32, 3);
    // keys.set_config(load_key_config, 38, 3);

    keys.set_code(KeyCodes::KeyboardLeftShift, 33, 3);
    keys.set_combined(KeyCodes::Layer2, KeyCodes::Layer3, 16, 34, 3);

    keys.set_debounce(18..36, false);
}
