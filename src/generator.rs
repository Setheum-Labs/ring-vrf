// Copyright (c) 2019-2020 Web 3 Foundation
//
// Authors:
// - Wei Tang <hi@that.world>
// - Sergey Vasilyev <swasilyev@gmail.com>

//! ### Ring VRF zkSNARK SRS generator


use bellman::groth16;

use crate::{rand_hack, JubjubEngineWithParams, SynthesisResult};
use group::WnafGroup;


/// Generates structured (meaning circuit-depending) Groth16
/// CRS (that comprises proving and verificaton keys) over BLS12-381
/// for the circuit defined in circuit.rs using OS RNG.
pub fn generate_crs<E: JubjubEngineWithParams>(depth: u32)
 -> SynthesisResult<groth16::Parameters<E>>
where
    E::G1: WnafGroup,
    E::G2: WnafGroup,
{
    let circuit = crate::circuit::RingVRF::<E> {
        depth,
        sk: None,
        vrf_input: None,
        extra: None,
        copath: None,
    };
    groth16::generate_random_parameters(circuit, &mut rand_hack())
}
