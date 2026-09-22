use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("mint carries an extension this program has not been written to handle")]
    UnsupportedExtension,
}