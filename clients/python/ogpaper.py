"""OG Paper automation client.

    pip install websockets

    import asyncio, ogpaper

    async def main():
        og = await ogpaper.connect("ws://nas:8991", "e...server key...")
        page = (await og.pages())[0]["page"]
        await og.run(page, [{"add": "text", "x": 0, "y": 0, "text": "Hello"}])
        print(await og.texts(page))
        await og.close()

    asyncio.run(main())

Commands and coordinates: see docs/PLUGINS.md.
"""

import asyncio
import itertools
import json

import websockets


class Client:
    def __init__(self, ws):
        self._ws = ws
        self._ids = itertools.count(1)
        self._waiting = {}
        self._watchers = {}
        self.can_edit = False
        self._reader = asyncio.ensure_future(self._read())

    async def _read(self):
        async for raw in self._ws:
            m = json.loads(raw)
            if m.get("event") == "changed":
                for f in self._watchers.get(m["page"], []):
                    f(m["page"])
                continue
            fut = self._waiting.pop(m.get("id"), None)
            if fut and not fut.done():
                if m.get("ok"):
                    fut.set_result(m)
                else:
                    fut.set_exception(RuntimeError(m.get("error")))

    async def call(self, **msg):
        i = next(self._ids)
        fut = asyncio.get_running_loop().create_future()
        self._waiting[i] = fut
        await self._ws.send(json.dumps({**msg, "id": i}))
        return await fut

    async def pages(self):
        return (await self.call(cmd="pages"))["pages"]

    async def new_page(self, name):
        return (await self.call(cmd="new_page", name=name))["page"]

    async def rename_page(self, page, name):
        await self.call(cmd="rename_page", page=page, name=name)

    async def delete_page(self, page):
        await self.call(cmd="delete_page", page=page)

    async def run(self, page, commands):
        return (await self.call(cmd="run", page=page, commands=commands))["results"]

    async def texts(self, page):
        return (await self.call(cmd="texts", page=page))["texts"]

    async def watch(self, page, fn):
        self._watchers.setdefault(page, []).append(fn)
        await self.call(cmd="watch", page=page)

    async def close(self):
        self._reader.cancel()
        await self._ws.close()


async def connect(url, key):
    ws = await websockets.connect(url.rstrip("/") + "/api")
    c = Client(ws)
    auth = await c.call(auth=key)
    c.can_edit = auth.get("can_edit", False)
    return c
