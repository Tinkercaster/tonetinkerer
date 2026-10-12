"""Export the best checkpoint of an (interrupted) nam-full run to a .nam file.

usage: .venv/bin/python export_ckpt.py <run_dir> <out_dir>
"""
import json
import sys
from pathlib import Path

from nam.train.lightning_module import LightningModule

run = Path(sys.argv[1])
out = Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
model_config = json.loads((run / "config_model.json").read_text())
ckpts = sorted(
    (run / "lightning_logs" / "version_0" / "checkpoints").glob("epoch=*_ESR=*.ckpt"),
    key=lambda p: float(p.name.split("ESR=")[1].split("_")[0]),
)
best = ckpts[0]
print("best checkpoint:", best.name)
model = LightningModule.load_from_checkpoint(best, **LightningModule.parse_config(model_config))
model.cpu()
model.eval()
model.net.sample_rate = 48000
model.net.export(out)
print("exported to", out)
