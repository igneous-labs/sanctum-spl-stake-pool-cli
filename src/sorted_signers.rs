use solana_sdk::{pubkey::Pubkey, signature::Signature, signer::Signer, signers::Signers};

/// newtype to impl Signers on to avoid lifetime errors from Vec::dedup()
pub struct SortedSigners<'slice, 'signer>(pub &'slice [&'signer dyn Signer]);

impl<'slice, 'signer> SortedSigners<'slice, 'signer> {
    pub fn iter(&self) -> SortedSignerIter<'_, '_, '_> {
        SortedSignerIter {
            inner: self,
            curr_i: 0,
        }
    }
}

pub struct SortedSignerIter<'a, 'slice, 'signer> {
    inner: &'a SortedSigners<'slice, 'signer>,
    curr_i: usize,
}

impl<'a, 'slice, 'signer> Iterator for SortedSignerIter<'a, 'slice, 'signer> {
    type Item = &'a dyn Signer;

    fn next(&mut self) -> Option<Self::Item> {
        let curr = self.inner.0.get(self.curr_i)?;
        let curr_pk = curr.pubkey();
        self.curr_i += 1;
        while let Some(next) = self.inner.0.get(self.curr_i) {
            if next.pubkey() != curr_pk {
                break;
            }
            self.curr_i += 1;
        }
        Some(*curr)
    }
}

impl<'slice, 'signer> Signers for SortedSigners<'slice, 'signer> {
    fn pubkeys(&self) -> Vec<Pubkey> {
        self.iter().map(|s| s.pubkey()).collect()
    }

    fn try_pubkeys(&self) -> Result<Vec<Pubkey>, solana_sdk::signer::SignerError> {
        self.iter().map(|s| s.try_pubkey()).collect()
    }

    fn sign_message(&self, message: &[u8]) -> Vec<Signature> {
        self.iter().map(|s| s.sign_message(message)).collect()
    }

    fn try_sign_message(
        &self,
        message: &[u8],
    ) -> Result<Vec<Signature>, solana_sdk::signer::SignerError> {
        self.iter().map(|s| s.try_sign_message(message)).collect()
    }

    fn is_interactive(&self) -> bool {
        self.iter().any(|s| s.is_interactive())
    }
}
