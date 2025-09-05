fn blake3_64_unkeyed(bytes: &[u8]) -> u64 {
    let digest = blake3::hash(bytes);
    u64::from_le_bytes(digest.as_bytes()[0..8].try_into().unwrap())
}

// Keyed variant (protects against chosen-key steering by clients):
fn blake3_64_keyed(bytes: &[u8], key32: [u8; 32]) -> u64 {
    let mut hasher = blake3::keyed_hash(&key32, bytes);
    u64::from_le_bytes(hasher.as_bytes()[0..8].try_into().unwrap())
}

pub struct HashRing {
    /// hash -> node_id
    mp_idx: Vec<(u64, usize)>,
    client_secret: [u8; 32],
}

impl HashRing {
    pub fn new(client_secret: [u8; 32]) -> Self {
        Self {
            mp_idx: Vec::new(),
            client_secret,
        }
    }

    pub fn build_mp<I, S>(&mut self, ite: I)
    where
        I: IntoIterator<Item = (S, usize)>,
        S: AsRef<str>,
    {
        let mut mp = Vec::new();
        for (ep, idx) in ite {
            let ep = ep.as_ref();
            let t = blake3_64_unkeyed(ep.as_bytes());
            mp.push((t, idx));
        }
        mp.sort_unstable_by_key(|&(t, _)| t);
        self.mp_idx = mp;
    }

    pub fn get_index(&self, client_id: &str) -> Option<usize> {
        if self.mp_idx.is_empty() {
            return None;
        }
        let h = blake3_64_keyed(client_id.as_bytes(), self.client_secret);
        match self.mp_idx.binary_search_by_key(&h, |&(t, _)| t) {
            Ok(pos) | Err(pos) => {
                let idx = if pos == self.mp_idx.len() { 0 } else { pos };
                Some(self.mp_idx[idx].1)
            }
        }
    }

    pub fn get_index_filtered<F>(&self, client_id: &str, filter: F) -> Option<usize>
    where
        F: Fn(usize) -> bool,
    {
        if self.mp_idx.is_empty() {
            return None;
        }
        let h = blake3_64_keyed(client_id.as_bytes(), self.client_secret);
        let start = match self.mp_idx.binary_search_by_key(&h, |&(t, _)| t) {
            Ok(pos) | Err(pos) => pos % self.mp_idx.len(),
        };

        for s in 0..self.mp_idx.len() {
            let idx = self.mp_idx[(start + s) % self.mp_idx.len()].1;
            if filter(idx) {
                return Some(idx);
            }
        }
        None
    }
}
