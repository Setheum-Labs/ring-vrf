// Copyright (c) 2019-2020 Web 3 Foundation
//
// Authors:
// - Jeffrey Burdges <jeff@web3.foundation>

//! ### VRF Output routines
//!
//! *Warning*  We warn that our ring VRF construction need malleable
//! outputs via the `*malleable*` methods.  These are insecure when
//! used in  conjunction with our HDKD provided in dervie.rs.
//! Attackers could translate malleable VRF outputs from one soft subkey 
//! to another soft subkey, gaining early knowledge of the VRF output.
//! We suggest using either non-malleable VRFs or using implicit
//! certificates instead of HDKD when using VRFs.

use std::io;

use rand_core::{RngCore,CryptoRng,SeedableRng};

use ff::{PrimeField, PrimeFieldRepr}; // Field, ScalarEngine
use zcash_primitives::jubjub::{JubjubEngine, Unknown, edwards::Point}; // PrimeOrder

use crate::{JubjubEngineWithParams, SigningTranscript, Params, Scalar};  // use super::*;


/// VRF input, always created locally from a `SigningTranscript`.
///
/// All creation methods require the developer acknoledge their VRF output malleability.
#[derive(Debug, Clone)]
pub struct VRFInput<E: JubjubEngine>(pub(crate) Point<E, Unknown>);

impl<E: JubjubEngineWithParams> VRFInput<E> {
    /// Create a new VRF input from an `RngCore`.
    #[inline(always)]
    fn from_rng<R: RngCore+CryptoRng>(mut rng: R) -> Self {
        let params = E::params();
        VRFInput( Point::rand(&mut rng, params).mul_by_cofactor(params).into() )
    }

    /// Acknoledge VRF transcript malleablity
    ///
    /// TODO: Verify that Point::rand is stable or find a stable alternative.
    pub fn new_malleable<T>(mut t: T) -> VRFInput<E> 
    where T: SigningTranscript
    {
        let mut seed = [0u8; 32]; // <ChaChaRng as rand_core::SeedableRng>::Seed::default();
        t.challenge_bytes(b"vrf-input", seed.as_mut());
        let rng = ::rand_chacha::ChaChaRng::from_seed(seed);
        VRFInput::from_rng(rng)
    }

    /// Non-malleable VRF transcript.
    ///
    /// Incompatable with ring VRF however.
    pub fn new_nonmalleable<T>(mut t: T, publickey: &crate::PublicKey<E>)
     -> VRFInput<E>
    where T: SigningTranscript
    {
        t.commit_point(b"vrf-nm-pk", &publickey.0);
        VRFInput::new_malleable(t)
    }

    /// Semi-malleable VRF transcript
    pub fn new_ring_malleable<T>(mut t: T, auth_root: &crate::merkle::AuthRoot<E>)
     -> VRFInput<E>
    where T: SigningTranscript
    {
        let mut buf = [0u8; 32];
        auth_root.0.into_repr()
        .write_le(&mut buf[..])
        .expect("Internal buffer write problem.  JubJub base field larger than 32 bytes?");
        t.commit_bytes(b"vrf-nm-ar", &buf);
        VRFInput::new_malleable(t)
    }

    /// Into VRF output.
    pub fn to_output(&self, sk: &crate::SecretKey<E>) -> VRFOutput<E> {
        VRFOutput( self.0.mul(sk.key.clone(), E::params()) )
    }

    pub fn to_inout(&self, sk: &crate::SecretKey<E>) -> VRFInOut<E> {
        let output = self.to_output(sk);
        VRFInOut { input: self.clone(), output }
    }

}


/// VRF output, possibly unverified.
#[derive(Debug, Clone)] // Default, PartialEq, Eq, PartialOrd, Ord, Hash
pub struct VRFOutput<E: JubjubEngine>(pub Point<E, Unknown>);

impl<E: JubjubEngineWithParams> VRFOutput<E> {
    pub fn read<R: io::Read>(reader: R) -> io::Result<Self> {
        let p = Point::read(reader,E::params()) ?;
        // ZCash has not method to check for a JubJub point being the identity,
        // but so long as the VRFInput can only be created by hashing, then this
        // sounds okay.
        // if p.is_identity() {
        //     return Err( io::Error::new(io::ErrorKind::InvalidInput, "Identity point provided as VRF output" ) );
        // }
        Ok(VRFOutput(p))
    }

    pub fn write<W: io::Write>(&self, writer: W) -> io::Result<()> {
        self.0.write(writer)
    }

    /// Acknoledge VRF transcript malleablity
    ///
    /// TODO: Verify that Point::rand is stable or find a stable alternative.
    pub fn attach_malleable<T>(&self, t: T) -> VRFInOut<E>
    where T: SigningTranscript
    {
        let input = VRFInput::new_malleable(t);
        VRFInOut { input, output: self.clone() }
    }

    /// Non-malleable VRF transcript.
    ///
    /// Incompatable with ring VRF however.
    pub fn attach_nonmalleable<T>(&self, t: T, publickey: &crate::PublicKey<E>)
     -> VRFInOut<E>
    where T: SigningTranscript
    {
        let input = VRFInput::new_nonmalleable(t,publickey);
        VRFInOut { input, output: self.clone() }
    }

    /// Semi-malleable VRF transcript
    pub fn attach_ring_malleable<T>(&self, t: T, auth_root: &crate::merkle::AuthRoot<E>)
     -> VRFInOut<E>
    where T: SigningTranscript
    {
        let input = VRFInput::new_ring_malleable(t,auth_root);
        VRFInOut { input, output: self.clone() }
    }
}


/// VRF input and output paired together, possibly unverified.
///
/// Internally, we keep both `RistrettoPoint` and `CompressedRistretto`
/// forms using `RistrettoBoth`.
#[derive(Debug, Clone)] // PartialEq, Eq, PartialOrd, Ord, Hash
pub struct VRFInOut<E: JubjubEngine> {
    /// VRF input point
    pub input: VRFInput<E>,
    /// VRF output point
    pub output: VRFOutput<E>,
}

impl<E: JubjubEngineWithParams> VRFInOut<E> {
    /// Write VRF output
    pub fn write_output<W: io::Write>(&self, writer: W) -> io::Result<()> {
        self.output.write(writer)
    }

    /// Commit VRF input and output to a transcript.
    ///
    /// We commit both the input and output to provide the 2Hash-DH
    /// construction from Theorem 2 on page 32 in appendix C of
    /// ["Ouroboros Praos: An adaptively-secure, semi-synchronous proof-of-stake blockchain"](https://eprint.iacr.org/2017/573.pdf)
    /// by Bernardo David, Peter Gazi, Aggelos Kiayias, and Alexander Russell.
    ///
    /// We use this construction both for the VRF usage methods
    /// `VRFInOut::make_*` as well as for signer side batching.
    pub fn commit<T: SigningTranscript>(&self, t: &mut T) {
        t.commit_point(b"vrf-in", &self.input.0);
        t.commit_point(b"vrf-out", &self.output.0);
    }

    /// Raw bytes output from the VRF.
    ///
    /// If you are not the signer then you must verify the VRF before calling this method.
    ///
    /// If called with distinct contexts then outputs should be independent.
    ///
    /// We incorporate both the input and output to provide the 2Hash-DH
    /// construction from Theorem 2 on page 32 in appendex C of
    /// ["Ouroboros Praos: An adaptively-secure, semi-synchronous proof-of-stake blockchain"](https://eprint.iacr.org/2017/573.pdf)
    /// by Bernardo David, Peter Gazi, Aggelos Kiayias, and Alexander Russell.
    pub fn make_bytes<B: Default + AsMut<[u8]>>(&self, context: &[u8]) -> B {
        let mut t = ::merlin::Transcript::new(b"VRFResult");
        t.append_message(b"",context);
        self.commit(&mut t);
        let mut seed = B::default();
        t.challenge_bytes(b"", seed.as_mut());
        seed
    }

    /// VRF output converted into any `SeedableRng`.
    ///
    /// If you are not the signer then you must verify the VRF before calling this method.
    ///
    /// We expect most users would prefer the less generic `VRFInOut::make_chacharng` method.
    pub fn make_rng<R: SeedableRng>(&self, context: &[u8]) -> R {
        R::from_seed(self.make_bytes::<R::Seed>(context))
    }

    /// VRF output converted into a `ChaChaRng`.
    ///
    /// If you are not the signer then you must verify the VRF before calling this method.
    ///
    /// If called with distinct contexts then outputs should be independent.
    /// Independent output streams are available via `ChaChaRng::set_stream` too.
    ///
    /// We incorporate both the input and output to provide the 2Hash-DH
    /// construction from Theorem 2 on page 32 in appendex C of
    /// ["Ouroboros Praos: An adaptively-secure, semi-synchronous proof-of-stake blockchain"](https://eprint.iacr.org/2017/573.pdf)
    /// by Bernardo David, Peter Gazi, Aggelos Kiayias, and Alexander Russell.
    #[cfg(feature = "rand_chacha")]
    pub fn make_chacharng(&self, context: &[u8]) -> ::rand_chacha::ChaChaRng {
        self.make_rng::<::rand_chacha::ChaChaRng>(context)
    }

    /// VRF output converted into Merlin's Keccek based `Rng`.
    ///
    /// If you are not the signer then you must verify the VRF before calling this method.
    ///
    /// We think this might be marginally slower than `ChaChaRng`
    /// when considerable output is required, but it should reduce
    /// the final linked binary size slightly, and improves domain
    /// separation.
    #[inline(always)]
    pub fn make_merlin_rng(&self, context: &[u8]) -> ::merlin::TranscriptRng {
        // Very insecure hack except for our commit_witness_bytes below
        struct ZeroFakeRng;
        impl RngCore for ZeroFakeRng {
            fn next_u32(&mut self) -> u32 {  panic!()  }
            fn next_u64(&mut self) -> u64 {  panic!()  }
            fn fill_bytes(&mut self, dest: &mut [u8]) {
                for i in dest.iter_mut() {  *i = 0;  }
            }
            fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), ::rand_core::Error> {
                self.fill_bytes(dest);
                Ok(())
            }
        }
        impl CryptoRng for ZeroFakeRng {}

        let mut t = ::merlin::Transcript::new(b"VRFResult");
        t.append_message(b"",context);
        self.commit(&mut t);
        t.build_rng().finalize(&mut ZeroFakeRng)
    }
}


/// Sample a 128 bit scalar for 
///
/// TODO: Improve this
fn challenge_scalar_128<E>(mut t: ::merlin::Transcript) -> Scalar<E> 
where E: JubjubEngine
{
    let mut s = [0u8; 16];
    t.challenge_bytes(b"", &mut s);
    let (x,y) = array_refs!(&s,8,8);
    let mut x: <E::Fs as PrimeField>::Repr = u64::from_le_bytes(*x).into();
    let y: <E::Fs as PrimeField>::Repr = u64::from_le_bytes(*y).into();
    x.shl(64);
    x.add_nocarry(&y);
    <E::Fs as PrimeField>::from_repr(x).unwrap()
}

/// Merge VRF input and output pairs from the same signer,
/// using variable time arithmetic
///
/// You should use `vartime=true` when verifying VRF proofs batched
/// by the singer.  You could usually use `vartime=true` even when
/// producing proofs, provided the set being signed is not secret.
///
/// There is sadly no constant time 128 bit multiplication in dalek,
/// making `vartime=false` somewhat slower than necessary.  It should
/// only impact signers in niche scenarios however, so the slower
/// variant should normally be unnecessary.
///
/// Panics if given an empty points list.
///
/// TODO: Add constant time 128 bit batched multiplication to dalek.
/// TODO: Is rand_chacha's `gen::<u128>()` standardizable enough to
/// prefer it over merlin for the output?  
pub fn vrfs_merge<E,B>(ps: &[B], params: Params<E>) -> VRFInOut<E>
where
    E: JubjubEngineWithParams,
    B: ::core::borrow::Borrow<VRFInOut<E>>,
{
    assert!( ps.len() > 0);
    let mut t = ::merlin::Transcript::new(b"MergeVRFs");
    for p in ps.iter() {
        p.borrow().commit(&mut t);
    }

    let go = |io: bool| {
        let mut acc = Point::zero();
        for p in ps {
            let mut t0 = t.clone();
            let p = p.borrow();
            p.commit(&mut t0);
            let z: Scalar<E> = challenge_scalar_128::<E>(t0);

            let p = if io { p.input.0.clone() } else { p.output.0.clone() };
            // acc += z * p;
            let p = p.mul(z,&params.engine);
            acc = acc.clone().add(&p, &params.engine);
        }
        acc
    };

    let input = VRFInput(go(true));
    let output = VRFOutput(go(false));
    VRFInOut { input, output }
}



#[cfg(test)]
mod tests {
}

