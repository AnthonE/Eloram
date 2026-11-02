//! Rerolls banked across matches, as League's ARAM banks them: a signed-in wallet holds up to
//! [`BANK_MAX`], spends one on each reroll in champion select, and earns one back for each match
//! it's still seated in when the match ends. Guests have no wallet to bank against: they get
//! [`GUEST_REROLLS`] a select. With a records directory the bank is kept in its `rerolls.json`,
//! so it outlives a restart.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use el_proto::auth::Address;

/// The most rerolls a wallet banks (and what one never seen before starts with).
pub const BANK_MAX: u8 = 2;
/// A guest's rerolls in each select.
pub const GUEST_REROLLS: u8 = 1;

#[derive(Default)]
pub struct Bank {
    /// Each wallet's rerolls, those not at [`BANK_MAX`] (a wallet not here has the most).
    held: Mutex<HashMap<Address, u8>>,
    file: Option<PathBuf>,
}

impl Bank {
    /// A bank kept in `file`, read back from it if it's there.
    pub fn kept(file: PathBuf) -> Self {
        let mut held = HashMap::new();
        if let Ok(b) = std::fs::read(&file)
            && let Ok(rows) = serde_json::from_slice::<Vec<(String, u8)>>(&b)
        {
            for (wallet, n) in rows {
                if let Some(a) = el_auth::parse_address(&wallet)
                    && n < BANK_MAX
                {
                    held.insert(a, n);
                }
            }
        }
        Self { held: Mutex::new(held), file: Some(file) }
    }

    /// The rerolls `wallet` has (a guest's, without one).
    pub fn balance(&self, wallet: Option<Address>) -> u8 {
        let Some(a) = wallet else { return GUEST_REROLLS };
        self.held.lock().map_or(BANK_MAX, |h| h.get(&a).copied().unwrap_or(BANK_MAX))
    }

    /// `wallet` spent one.
    pub fn spend(&self, wallet: Address) {
        self.change(wallet, |n| n.saturating_sub(1));
    }

    /// `wallet` earned one (up to the most).
    pub fn earn(&self, wallet: Address) {
        self.change(wallet, |n| (n + 1).min(BANK_MAX));
    }

    fn change(&self, wallet: Address, f: impl FnOnce(u8) -> u8) {
        let Ok(mut h) = self.held.lock() else { return };
        let was = h.get(&wallet).copied().unwrap_or(BANK_MAX);
        let now = f(was);
        if now == was {
            return;
        }
        if now >= BANK_MAX {
            h.remove(&wallet);
        } else {
            h.insert(wallet, now);
        }
        if let Some(file) = &self.file {
            let mut rows: Vec<(String, u8)> = h
                .iter()
                .map(|(a, n)| (String::from_utf8_lossy(&el_auth::lower_hex(a)).into_owned(), *n))
                .collect();
            rows.sort();
            if let Ok(b) = serde_json::to_vec(&rows) {
                crate::records::write_atomically(file, &b);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wallet_spends_and_earns_up_to_the_most_and_its_bank_outlives_a_restart() {
        let dir = std::env::temp_dir().join(format!("el-rerolls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("rerolls.json");
        let _ = std::fs::remove_file(&file);
        let a = Address([0xab; 20]);
        let bank = Bank::kept(file.clone());
        assert_eq!(bank.balance(Some(a)), BANK_MAX, "a new wallet starts full");
        assert_eq!(bank.balance(None), GUEST_REROLLS);
        bank.spend(a);
        bank.spend(a);
        bank.spend(a);
        assert_eq!(bank.balance(Some(a)), 0, "never below none");
        bank.earn(a);
        let again = Bank::kept(file.clone());
        assert_eq!(again.balance(Some(a)), 1, "kept across a restart");
        again.earn(a);
        again.earn(a);
        assert_eq!(again.balance(Some(a)), BANK_MAX, "never past the most");
        assert_eq!(Bank::kept(file).balance(Some(a)), BANK_MAX);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
