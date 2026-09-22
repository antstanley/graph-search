"""A small module exercising the Python extractor's constructs."""

from __future__ import annotations

import os
import os.path as osp
from . import helpers
from .models import User, Account as Acct
from typing import Optional

MAX = 100

type Alias = list[int]


class Base:
    def ping(self) -> None:
        pass


class Widget(Base):
    size: int = 0
    name = "widget"

    def __init__(self, size: int) -> None:
        self.size = size

    async def render(self) -> Optional[str]:
        self.draw()
        return osp.join(self.name, "x")


def make_widget() -> Widget:
    return Widget(1)


def run() -> None:
    widget = make_widget()
    widget.render()
    helpers.help()
    Acct.open()
