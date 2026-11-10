#!/usr/bin/env python3
"""End-to-end test of the CMR fleet features (src/server/src/cmr.rs, intents/cmr.rs) against two CHR VMs:

  CMR server:  CHR_DIR=~/.cache/tikontrol-cmr        CHR_OFFSET=100  (7.26+ with the cmr package, `/cmr set enabled=yes`,
               started with CHR_FWD=,hostfwd=tcp:127.0.0.1:54321-:54321)
  managed device: CHR_DIR=~/.cache/tikontrol-cmr-client CHR_OFFSET=300 (7.26+, `/cmr/client set enabled=yes controller-addresses=10.0.2.2`)

tikontrol connects to the device as its router (one-step setup) and to the server as a separate CMR controller
with its own reader and writer users, then drives every cmr-* intent through the HTTP API like the UI.
Each change is checked with the server's `/export terse`: apply must change it, revert must restore it exactly.

    python3 src/scripts/cmr-e2e.py [--bin target/release/tikontrol]

Never point this at real routers: it writes, and it unpairs the managed device to test adoption.
"""
import argparse, importlib.util, json, os, secrets, shutil, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("chr_e2e", os.path.join(HERE, "chr-e2e.py"))
e2e = importlib.util.module_from_spec(spec)
spec.loader.exec_module(e2e)
print = e2e.print
# its own port, so it can run next to chr-e2e.py
e2e.PORT = 9543
e2e.BASE = f"https://127.0.0.1:{e2e.PORT}"

SRV = os.path.expanduser(os.environ.get("CMR_DIR", "~/.cache/tikontrol-cmr"))
DEV = os.path.expanduser(os.environ.get("CMR_CLIENT_DIR", "~/.cache/tikontrol-cmr-client"))
SRV_API, DEV_API = 8729 + 100, 8729 + 300


def ros(d, cmd):
    return subprocess.run([os.path.join(d, "ssh-chr.sh"), cmd], capture_output=True, text=True, timeout=120).stdout


def srv_export():
    lines = [l.rstrip() for l in ros(SRV, "/export terse").replace("\r", "").split("\n") if l.strip()]
    # `disabled` can't be unset on /cmr/alert: an explicit disabled=no is the default, not a difference
    return [l.replace(" disabled=no", "") for l in lines if not e2e.VOLATILE.search(l)]


def wait(pred, what, secs=90):
    for _ in range(secs // 3):
        if pred():
            return
        time.sleep(3)
    sys.exit(f"timed out waiting for {what}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=os.path.join(HERE, "..", "server", "target", "release", "tikontrol"))
    a = ap.parse_args()
    data = tempfile.mkdtemp(prefix="tik-cmr-e2e-")
    env = dict(os.environ, TIK_DATA=data, TIK_LISTEN=f"127.0.0.1:{e2e.PORT}", TIK_ROUTER="127.0.0.1", TIK_POLL="30")
    log = open(os.path.join(SRV, "e2e-server.log"), "w")
    srv = subprocess.Popen([a.bin], env=env, stdout=log, stderr=subprocess.STDOUT)
    fails = 0

    def check(name, ok, msg=""):
        nonlocal fails
        fails += not ok
        print(f"{'PASS' if ok else 'FAIL'} {name:46} {'' if ok else msg}")

    try:
        time.sleep(1.5)
        # leftovers of an interrupted run (the VMs can panic under host load)
        ros(SRV, '/cmr/alert remove [find comment~"^tik:alert:tik-e2e"]; /cmr/upgrade remove [find comment~"^tik:upgrade:"]; /cmr/wifi remove [find comment="test: cmr-e2e"]; /system/scheduler remove [find name~"^tik-revert-"]')
        ident = ros(DEV, ":put [/system/identity get name]").strip()
        # controller users: a reader with no write/sensitive policy, and a writer (pairing needs `reboot`); fresh passwords each run
        rpw, wpw = secrets.token_hex(12), secrets.token_hex(12)
        for name, pol, pw in (("tik-cmr-read", "read,api,test", rpw), ("tik-cmr-write", "read,write,api,test,reboot", wpw)):
            ros(SRV, f'/user remove [find name={name}]; /user/group remove [find name={name}]; /user/group add name={name} policy={pol}; /user add name={name} group={name} password="{pw}"')
        c = e2e.Client()
        # TCG VMs sometimes reboot (softlockup panic under host load): wait until the device's API answers
        for _ in range(60):
            s, p = c.call("POST", "/api/v1/setup/probe", {"host": "127.0.0.1", "port": DEV_API})
            if s == 200:
                break
            time.sleep(3)
        assert s == 200, p
        s, r = c.call("POST", "/api/v1/setup/router", {"host": "127.0.0.1", "port": DEV_API, "pin": p["pin"], "user": "admin", "password": open(os.path.join(DEV, "admin.pw")).read().strip()})
        assert s == 200, r
        c.call("GET", "/api/v1/me")
        s, p = c.call("POST", "/api/v1/router/probe", {"host": "127.0.0.1", "port": SRV_API})
        assert s == 200, p
        ctl = {"host": "127.0.0.1", "port": SRV_API, "pin": p["pin"], "reader": {"user": "tik-cmr-read", "password": rpw}}
        s, r = c.call("POST", "/api/v1/controller", dict(ctl, reader={"user": "tik-cmr-write", "password": wpw}))
        check("controller refuses a reader that can write", s == 400, f"{s} {r}")
        s, r = c.call("POST", "/api/v1/controller", ctl)
        check("controller (read-only)", s == 200, f"{s} {r}")
        s, r = c.call("POST", "/api/v1/apply/preview", {"intent": {"kind": "cmr-alert", "name": "tik-e2e", "metric": "cpu", "threshold": 1}})
        check("read-only controller: preview only", s == 200 and r["canApply"] is False, f"{s} {r}")
        s, r = c.call("POST", "/api/v1/controller", dict(ctl, writer={"user": "tik-cmr-write", "password": wpw}, confirm_write=True))
        check("controller with a writer", s == 200, f"{s} {r}")

        # adoption: unpair the device so it comes back pending, then adopt it through Safe apply
        ros(SRV, f'/cmr/device remove [find identity="{ident}"]')
        cmr = lambda: c.call("GET", "/api/v1/cmr")[1]
        dev = lambda: next((d for d in cmr().get("devices", []) if d.get("identity") == ident), None)
        wait(lambda: (dev() or {}).get("paired") is False, "the device to come back pending")
        ids = dev()["ids"]
        pair = {"kind": "cmr-pair", "device": ids, "username": "admin", "password": open(os.path.join(DEV, "admin.pw")).read().strip()}
        before = srv_export()
        st, msg = c.change(pair, None)
        during = srv_export()
        check("cmr-pair applies", st == "ok" and during != before and any(ident in l for l in during), f"{st} {msg}")
        if st == "ok":
            s, r = c.call("POST", "/api/v1/apply/revert", {"id": msg["id"]})
            check("cmr-pair reverts (unpairs)", s == 200 and srv_export() == before, f"{s} {r} {e2e.diff(before, srv_export())}")
        wait(lambda: (dev() or {}).get("paired") is False, "pending again")
        st, msg = c.change(pair, "keep")
        wait(lambda: (dev() or {}).get("paired") is True and dev().get("online") is True, "the adopted device online")
        check("cmr-pair keep: adopted and online", st == "ok")

        c.change({"kind": "cmr-alert", "name": "tik-e2e toggle", "metric": "update"}, "keep")
        cases = [
            ({"kind": "cmr-device-labels", "device": ids, "labels": ["ap", "office"]}, None, "ok"),
            ({"kind": "cmr-upgrade-rule", "name": "nightly-aps", "labels": ["ap"], "channel": "stable", "time": "03:00", "days": ["sat", "sun"], "strategy": "sequential"}, {"kind": "cmr-remove-upgrade-rule", "name": "nightly-aps"}, "ok"),
            ({"kind": "cmr-alert", "name": "tik-e2e cpu", "metric": "cpu", "threshold": 95, "severity": "high"}, {"kind": "cmr-remove-alert", "name": "tik-e2e cpu"}, "ok"),
            ({"kind": "cmr-alert", "name": "tik-e2e offline", "metric": "offline", "threshold": 60, "labels": ["ap"]}, {"kind": "cmr-remove-alert", "name": "tik-e2e offline"}, "ok"),
            # the built-in example alerts would show up in export once touched, so toggle one of ours
            ({"kind": "cmr-alert-enabled", "name": "tik-e2e toggle", "enabled": False}, None, "ok"),
            ({"kind": "cmr-upgrade-device", "device": ids}, None, "error"),
            ({"kind": "cmr-pair", "device": ids}, None, "error"),
            ({"kind": "cmr-remove-upgrade-rule", "name": "default"}, None, "error"),
        ]
        for intent, inverse, want in cases:
            name = intent["kind"] + ("" if not inverse else " + " + inverse["kind"])
            before = srv_export()
            if want == "error":
                st, msg = c.change(intent, "revert")
                check(name, st in ("preview", "apply") and srv_export() == before, f"expected a refusal, got {st} {msg}")
                continue
            st, msg = c.change(intent, None)
            changed = srv_export() != before
            if st != "ok" or not changed:
                check(name, False, f"{st} {msg} changed={changed}")
                continue
            s, r = c.call("POST", "/api/v1/apply/revert", {"id": msg["id"]})
            problems = [] if srv_export() == before else ["revert left: " + "; ".join(e2e.diff(before, srv_export())[:6])]
            if inverse and not problems:
                st, _ = c.change(intent, "keep")
                st2, m2 = c.change(inverse, "keep")
                if st2 != "ok" or srv_export() != before:
                    problems.append(f"{inverse['kind']} after keep: {st2} {m2} {e2e.diff(before, srv_export())[:6]}")
            check(name, not problems, "; ".join(problems))

        c.change({"kind": "cmr-remove-alert", "name": "tik-e2e toggle"}, "keep")
        # Supervisor: an always-on state alert shows which devices it fires for
        st, _ = c.change({"kind": "cmr-alert", "name": "tik-e2e busy", "metric": "memory", "threshold": 10}, "keep")
        busy = lambda: next((x for x in cmr().get("alerts", []) if x.get("name") == "tik-e2e busy"), {})
        wait(lambda: busy().get("active"), "the supervisor alert to fire", 120)
        check("supervisor: active alert lists devices", bool(busy().get("active")), str(busy()))
        c.change({"kind": "cmr-remove-alert", "name": "tik-e2e busy"}, "keep")
        # labels → which APs a CMR Wi-Fi network selects (a CHR has no radio: selected, nothing broadcast)
        c.change({"kind": "cmr-device-labels", "device": ids, "labels": ["ap"]}, "keep")
        ros(SRV, '/cmr/wifi add ssid=tik-e2e labels=ap,-core comment="test: cmr-e2e" security.authentication-types=wpa2-psk security.passphrase=not-a-real-pass-1')
        v = cmr()
        w = next((x for x in v.get("wifi", []) if x.get("ssid") == "tik-e2e"), {})
        check("wifi: selected APs from labels", w.get("aps") == [ident], str(w))
        check("wifi: passphrase never read", "not-a-real-pass" not in json.dumps(v))
        ros(SRV, '/cmr/wifi remove [find comment="test: cmr-e2e"]')
        c.change({"kind": "cmr-device-labels", "device": ids, "labels": []}, "keep")
        # app traffic: some traffic through the device, then the classifier's table
        ros(DEV, "/tool fetch url=https://upgrade.mikrotik.com/routeros/NEWESTa7.stable output=none")
        https = lambda: any(x["name"] == "HTTPS" and (x["hour"] or [0])[0] > 0 for x in cmr().get("apps", []))
        for _ in range(50):  # the classifier reports to the server about once a minute
            if https():
                break
            time.sleep(3)
        v = cmr()
        check("apptraffic: top applications", https(), str((v.get("apps"), v.get("appsError")))[:300])
        check("devices: controller + device online", sum(1 for d in cmr()["devices"] if d["online"]) >= 2)
    finally:
        srv.terminate()
        shutil.rmtree(data, ignore_errors=True)
        for name in ("tik-cmr-read", "tik-cmr-write"):
            ros(SRV, f"/user remove [find name={name}]; /user/group remove [find name={name}]")
    print(f"\n{fails} failure(s)")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
