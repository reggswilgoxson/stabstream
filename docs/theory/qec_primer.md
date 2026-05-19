# QEC Primer: Stabilizer Codes and Syndrome Measurement

## Stabilizer Codes

A stabilizer code encodes $k$ logical qubits into $n$ physical qubits. The code
space is defined as the joint +1 eigenspace of a set of Pauli operators called
**stabilizers**. For a surface code with distance $d$, there are $d^2$ data
qubits, $(d^2 - 1)$ stabilizers, and 1 logical qubit.

Each stabilizer is a tensor product of Pauli operators (I, X, Y, Z) acting on a
subset of data qubits. By convention:

- **X-type stabilizers** detect Z errors
- **Z-type stabilizers** detect X errors

## Syndrome Measurement

To detect errors without disturbing the logical state, each stabilizer is
measured indirectly via an **ancilla qubit**:

```
data qubits ──┬──┬──  stabilizer S = X₀X₁Z₂Z₃
              │  │
ancilla  ──H──●──●──H──M   (for X-type; CNOT targets swapped for Z-type)
```

A measurement outcome of −1 indicates the data qubits have been driven out of
the stabilizer's +1 eigenspace by an error. This is a **detector event**.

**Key invariant**: in the absence of errors, every stabilizer measurement returns
+1. A pattern of −1 outcomes (the **syndrome**) points to the location and type
of errors.

## The Decoding Problem

Given a syndrome $\sigma \in \{0,1\}^{n-k}$ (one bit per stabilizer), find a
Pauli correction $C$ such that $C \cdot E \in \mathcal{S}$, where $E$ is the
unknown physical error and $\mathcal{S}$ is the stabilizer group.

This is equivalent to finding a minimum-weight correction consistent with the
measured syndrome. The failure mode — a **logical error** — occurs when the
correction differs from the true error by a logical operator (one that commutes
with all stabilizers but is not itself a stabilizer).

## Spacetime Syndrome Graphs

Real hardware repeats stabilizer measurements over many rounds to distinguish
transient measurement errors from data-qubit errors. This turns the decoding
problem three-dimensional:

```
round t   ●─────●─────●    ● = ancilla measurement outcome
          │     │     │    ─ = "likely measurement error" edge
round t+1 ●─────●─────●    │ = "likely data error" edge
          │     │     │
round t+2 ●─────●─────●
```

A **Stim Detector Error Model (DEM)** encodes this graph: each `error(p)` line
names the detector nodes a single fault mechanism connects, and the Pauli
observables it flips. Edge weights are $-\ln\!\left(\frac{p}{1-p}\right)$.

## Logical Error Rate and Threshold

The fundamental QEC metric is the **logical error rate** $p_L$: the probability
per shot that the decoder makes an incorrect logical correction.

For a code of distance $d$ under physical error rate $p$:

$$p_L \approx A \left(\frac{p}{p_\text{th}}\right)^{\lfloor d/2 \rfloor + 1}$$

Below the **threshold** $p_\text{th}$, increasing $d$ exponentially suppresses
$p_L$. Above threshold, increasing $d$ makes things worse.

For the rotated surface code with circuit-level noise, the threshold is
approximately $p_\text{th} \approx 1\%$. IBM's Bivariate Bicycle (BB) codes
have thresholds in the same range but with much higher encoding rates
($k/n \approx 8\%$ vs $1/d^2$).

## stabstream's Role

stabstream operates at the interface between hardware and decoder:

```
Quantum hardware
  → QSSF binary frames (detector events, measurement outcomes)
    → SyndromeWindow (sliding $d$-round buffer)
      → Decoder (Union-Find, PyMatching, neural)
        → LogicalCorrection (which logical operators to apply)
          → LogicalErrorAccumulator (track p_L over time)
```

The QSSF format is the lingua franca: it carries detector events from any source
(Stim simulation, IBM hardware, Google hardware) in a compact binary format
that stabstream parses at ~600 ns/frame.
