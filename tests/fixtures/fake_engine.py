import json
import sys
import time

args = sys.argv[1:]
mode = args[args.index("--name") + 1] if "--name" in args else "ok"

if mode == "ok":
    print(json.dumps({"v": 1, "event": "start", "command": "backup", "name": "fake"}))
    print(json.dumps({"v": 1, "event": "result", "ok": True, "name": "fake", "size": 1, "verified": True}))
elif mode == "garbage":
    print("{not json")
    print(json.dumps({"v": 1, "event": "result", "ok": True, "name": "fake", "size": 1, "verified": True}))
elif mode == "huge":
    print("x" * 70000)
elif mode == "nonutf8":
    sys.stdout.buffer.write(b'{"v":1,"event":"log","message":"\xff\xfe"}\n')
    sys.stdout.buffer.flush()
elif mode == "silent":
    sys.exit(3)
elif mode == "hang":
    time.sleep(60)
