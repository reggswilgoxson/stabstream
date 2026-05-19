"""
stabstream.vendors — hardware-vendor SDK adapters.

Each submodule converts vendor-specific result objects into stabstream
``SyndromeFrame``-compatible dicts (or NumPy arrays) that can be fed
directly into ``SyndromeWindow.push_numpy()`` or ``pd.DataFrame()``.

Available adapters
------------------
stabstream.vendors.ibm
    Converts Qiskit Runtime ``SamplerV2 / SamplerPubResult`` to frame dicts.
stabstream.vendors.cirq
    Converts Google Cirq ``Result`` / ``SimulationResult`` to frame dicts.
"""
