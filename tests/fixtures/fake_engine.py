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
elif mode == "graceful":
    import signal

    print(json.dumps({"v": 1, "event": "start", "command": "backup", "name": "fake"}))
    sys.stdout.flush()

    def stop(signum, frame):
        print(json.dumps({"v": 1, "event": "cancelled", "reason": "user"}))
        sys.stdout.flush()
        sys.exit(0)

    signal.signal(signal.SIGINT, stop)
    while True:
        time.sleep(0.1)
