"""
stabstream.plot — matplotlib helpers for QEC data visualization.

Functions
---------
plot_syndrome_heatmap       Detector events heatmap (rounds × ancillas)
plot_threshold_curves       p_L vs p curves for multiple code distances
plot_fire_frequency         Per-ancilla firing frequency bar chart / 2-D layout
plot_syndrome_weight_hist   Syndrome weight distribution histogram
plot_latency_hist           Decode latency distribution (one series per decoder)
"""

from __future__ import annotations

from typing import Any

import numpy as np

try:
    import matplotlib.pyplot as plt
    import matplotlib.colors as mcolors
    _MPL_AVAILABLE = True
except ImportError:
    _MPL_AVAILABLE = False


def _require_mpl() -> None:
    if not _MPL_AVAILABLE:
        raise ImportError("matplotlib is required: pip install matplotlib")


def plot_syndrome_heatmap(
    matrix: np.ndarray,
    *,
    ancilla_labels: list[str] | None = None,
    ax: Any = None,
    title: str = "Detector Events",
    cmap: str = "Blues",
) -> Any:
    """
    Plot a heatmap of detector events.

    Parameters
    ----------
    matrix : np.ndarray
        Shape ``(rounds, ancillas)``, dtype bool or int.
    ancilla_labels : list[str], optional
        X-axis tick labels.  Defaults to ``A0, A1, ...``.
    ax : matplotlib.axes.Axes, optional
        Axes to draw on.  Creates a new figure if ``None``.
    title : str
    cmap : str
        Matplotlib colormap name.

    Returns
    -------
    matplotlib.axes.Axes
    """
    _require_mpl()
    if ax is None:
        _, ax = plt.subplots(figsize=(max(6, matrix.shape[1] * 0.35), 4))

    rounds, n_ancilla = matrix.shape
    im = ax.imshow(matrix.astype(float), aspect="auto", cmap=cmap,
                   vmin=0, vmax=1, interpolation="nearest")
    plt.colorbar(im, ax=ax, label="Detector fired")

    ax.set_xlabel("Ancilla index")
    ax.set_ylabel("Round")
    ax.set_title(title)

    if ancilla_labels is not None:
        ax.set_xticks(range(n_ancilla))
        ax.set_xticklabels(ancilla_labels, rotation=45, ha="right", fontsize=8)
    elif n_ancilla <= 30:
        ax.set_xticks(range(n_ancilla))
        ax.set_xticklabels([f"A{i}" for i in range(n_ancilla)],
                           rotation=45, ha="right", fontsize=8)

    return ax


def plot_threshold_curves(
    data: list[dict],
    *,
    ax: Any = None,
    title: str = "QEC Threshold",
    show_diagonal: bool = True,
) -> Any:
    """
    Plot logical error rate p_L vs physical error rate p for multiple distances.

    Parameters
    ----------
    data : list[dict]
        Each dict must have keys ``distance`` (int), ``p_physical`` (float),
        ``p_l`` (float).  Optionally ``p_l_err`` (float) for error bars.
        This is the same format as ``stabstream-threshold`` JSON output.
    ax : matplotlib.axes.Axes, optional
    title : str
    show_diagonal : bool
        Draw the y=x reference line (above threshold region).

    Returns
    -------
    matplotlib.axes.Axes
    """
    _require_mpl()
    if ax is None:
        _, ax = plt.subplots(figsize=(7, 5))

    from collections import defaultdict
    by_dist: dict[int, list[dict]] = defaultdict(list)
    for pt in data:
        by_dist[pt["distance"]].append(pt)

    palette = ["#E6194B", "#4363D8", "#3CB44B", "#F58231", "#911EB4", "#42D4F4"]
    for idx, (dist, pts) in enumerate(sorted(by_dist.items())):
        pts_sorted = sorted(pts, key=lambda p: p["p_physical"])
        xs = [p["p_physical"] for p in pts_sorted]
        ys = [p["p_l"] for p in pts_sorted]
        errs = [p.get("p_l_err", 0.0) for p in pts_sorted]
        color = palette[idx % len(palette)]

        ax.plot(xs, ys, "-o", color=color, label=f"d = {dist}", linewidth=2, markersize=5)
        if any(e > 0 for e in errs):
            ax.errorbar(xs, ys, yerr=errs, fmt="none", color=color, capsize=3, alpha=0.6)

    if show_diagonal:
        lim = ax.get_xlim()
        diag = [min(lim), max(lim)]
        ax.plot(diag, diag, "--", color="gray", linewidth=1, alpha=0.5, label="p_L = p")

    ax.set_xlabel("Physical error rate  p")
    ax.set_ylabel("Logical error rate  p_L")
    ax.set_title(title)
    ax.legend(framealpha=0.9)
    ax.grid(True, alpha=0.3)
    return ax


def plot_fire_frequency(
    freqs: np.ndarray,
    *,
    layout: np.ndarray | None = None,
    ax: Any = None,
    title: str = "Per-Ancilla Fire Frequency",
    expected_rate: float | None = None,
    outlier_threshold: float = 3.0,
) -> Any:
    """
    Plot the per-ancilla firing frequency.

    Parameters
    ----------
    freqs : np.ndarray
        Shape ``(ancilla_count,)``.  Values in [0, 1].
    layout : np.ndarray, optional
        Shape ``(ancilla_count, 2)`` — (x, y) grid positions for a 2-D
        heatmap.  If ``None``, draws a bar chart.
    ax : matplotlib.axes.Axes, optional
    title : str
    expected_rate : float, optional
        Draw a horizontal reference line at this rate (typically ~2p).
    outlier_threshold : float
        Flag ancillas whose frequency deviates by more than
        ``outlier_threshold`` standard deviations from the mean.

    Returns
    -------
    matplotlib.axes.Axes
    """
    _require_mpl()
    n = len(freqs)
    mean_f = float(np.mean(freqs))
    std_f = float(np.std(freqs))
    outliers = np.abs(freqs - mean_f) > outlier_threshold * std_f

    if layout is not None:
        if ax is None:
            _, ax = plt.subplots(figsize=(6, 6))
        xs, ys = layout[:, 0], layout[:, 1]
        sc = ax.scatter(xs, ys, c=freqs, cmap="RdYlGn_r", s=200,
                        vmin=0, vmax=max(freqs.max(), mean_f + 3 * std_f + 1e-9))
        plt.colorbar(sc, ax=ax, label="Fire frequency")
        for i in np.where(outliers)[0]:
            ax.scatter(xs[i], ys[i], s=300, facecolors="none",
                       edgecolors="red", linewidths=2, zorder=5)
        ax.set_xlabel("x")
        ax.set_ylabel("y")
    else:
        if ax is None:
            _, ax = plt.subplots(figsize=(max(6, n * 0.4), 4))
        colors = ["#d62728" if o else "#1f77b4" for o in outliers]
        ax.bar(range(n), freqs, color=colors, width=0.8)
        if expected_rate is not None:
            ax.axhline(expected_rate, color="green", linestyle="--",
                       linewidth=1.5, label=f"Expected ~{expected_rate:.3f}")
            ax.legend()
        ax.axhline(mean_f, color="black", linestyle=":", linewidth=1, alpha=0.6)
        ax.set_xlabel("Ancilla index")
        ax.set_ylabel("Fire frequency")
        ax.set_xlim(-0.5, n - 0.5)

    ax.set_title(title)
    return ax


def plot_syndrome_weight_hist(
    weights: np.ndarray | list[int],
    *,
    ax: Any = None,
    title: str = "Syndrome Weight Distribution",
    max_weight: int | None = None,
) -> Any:
    """
    Plot the distribution of per-shot syndrome weights.

    Parameters
    ----------
    weights : array-like
        1-D array of syndrome weights (number of fired detectors per shot).
    ax : matplotlib.axes.Axes, optional
    title : str
    max_weight : int, optional
        Clip the x-axis at this weight.

    Returns
    -------
    matplotlib.axes.Axes
    """
    _require_mpl()
    weights = np.asarray(weights)
    if max_weight is None:
        max_weight = int(weights.max()) + 1

    if ax is None:
        _, ax = plt.subplots(figsize=(7, 4))

    bins = np.arange(-0.5, max_weight + 1.5)
    ax.hist(weights, bins=bins, color="#4363D8", edgecolor="white", linewidth=0.5)
    ax.set_xlabel("Syndrome weight (# fired detectors)")
    ax.set_ylabel("Shots")
    ax.set_title(title)
    ax.set_xlim(-0.5, max_weight + 0.5)
    ax.grid(True, axis="y", alpha=0.3)
    return ax


def plot_latency_hist(
    latencies: dict[str, np.ndarray],
    *,
    ax: Any = None,
    title: str = "Decode Latency Distribution",
    bins: int = 50,
    log_x: bool = True,
) -> Any:
    """
    Overlay latency distributions for multiple decoders.

    Parameters
    ----------
    latencies : dict[str, np.ndarray]
        Mapping of decoder name → 1-D array of latencies in nanoseconds.
    ax : matplotlib.axes.Axes, optional
    title : str
    bins : int
    log_x : bool
        Use a logarithmic x-axis (recommended for ns-scale latencies).

    Returns
    -------
    matplotlib.axes.Axes
    """
    _require_mpl()
    if ax is None:
        _, ax = plt.subplots(figsize=(7, 4))

    palette = ["#4363D8", "#E6194B", "#3CB44B", "#F58231"]
    for idx, (name, lats) in enumerate(latencies.items()):
        lats = np.asarray(lats, dtype=float)
        if log_x:
            lats = lats[lats > 0]
            bins_arr = np.logspace(np.log10(lats.min()), np.log10(lats.max()), bins)
        else:
            bins_arr = bins
        ax.hist(lats, bins=bins_arr, alpha=0.65,
                label=f"{name}  (p50={np.percentile(lats, 50):.0f} ns)",
                color=palette[idx % len(palette)], edgecolor="none")

    if log_x:
        ax.set_xscale("log")
    ax.set_xlabel("Decode latency (ns)")
    ax.set_ylabel("Count")
    ax.set_title(title)
    ax.legend(framealpha=0.9)
    ax.grid(True, alpha=0.3)
    return ax
