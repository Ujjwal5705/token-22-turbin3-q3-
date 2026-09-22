use anchor_lang::prelude::*;

// No custom account state needed yet — Token-2022 mint/account state is
// read via StateWithExtensions directly, not through an Anchor #[account].