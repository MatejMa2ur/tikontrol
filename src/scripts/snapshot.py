#!/usr/bin/env python3
"""Read-only snapshot of a RouterOS router into JSON for the UI mockup.

ponytail: SSH + :serialize is a mockup data source only; the real backend uses the binary API.
Usage: src/scripts/snapshot.py [user@host] > src/ui/src/lib/snapshot.json
"""
import json, re, subprocess, sys

HOST = sys.argv[1] if len(sys.argv) > 1 else "admin@192.168.88.1"

# key -> RouterOS expression. proplists are explicit so secrets (keys, passphrases) never leave the router.
Q = {
    "identity": "[/system/identity/get name]",
    "resource": "[/system/resource/get]",
    "routerboard": '({"model"=[/system/routerboard/get model];"current-firmware"=[/system/routerboard/get current-firmware];"upgrade-firmware"=[/system/routerboard/get upgrade-firmware]})',
    "update": '({"channel"=[/system/package/update/get channel];"installed-version"=[/system/package/update/get installed-version];"latest-version"=[/system/package/update/get latest-version]})',
    "wan": "[/ip/dhcp-client/print as-value proplist=interface,address,gateway,status]",
    # never /ip/cloud/get without a field name: it returns the BTH private key
    "cloud": '({"public-address"=[/ip/cloud/get public-address];"dns-name"=[/ip/cloud/get dns-name];"back-to-home-vpn"=[/ip/cloud/get back-to-home-vpn];"ddns-enabled"=[/ip/cloud/get ddns-enabled]})',
    "wanTraffic": "[/interface/monitor-traffic ether1 once as-value]",
    "interfaces": "[/interface/print as-value proplist=name,type,running,disabled,rx-byte,tx-byte,comment,mac-address]",
    "ethernet": "[/interface/ethernet/print as-value proplist=name,running,disabled]",
    "wifi": "[/interface/wireless/print as-value proplist=name,ssid,band,frequency,channel-width,running,disabled,mode,master-interface,security-profile,hide-ssid,default-forwarding,default-ap-tx-limit,default-client-tx-limit,comment]",
    "stations": "[/interface/wireless/registration-table/print as-value proplist=interface,mac-address,signal-strength,tx-rate,rx-rate,uptime,bytes,last-ip]",
    "leases": "[/ip/dhcp-server/lease/print as-value proplist=address,mac-address,host-name,comment,status,dynamic,last-seen,class-id]",
    "arp": "[/ip/arp/print as-value proplist=address,mac-address,interface,status]",
    "bridgeHosts": "[/interface/bridge/host/print as-value proplist=mac-address,interface,on-interface,local]",
    "dns": '({"cache-size"=[/ip/dns/get cache-size];"cache-used"=[/ip/dns/get cache-used]})',
    "adlist": "[/ip/dns/adlist/print as-value proplist=url,name-count,match-count,disabled]",
    "filter": "[/ip/firewall/filter/print as-value proplist=chain,action,comment,disabled,in-interface-list,connection-state,src-mac-address,src-address,src-address-list,protocol,tls-host,time]",
    "nat": "[/ip/firewall/nat/print as-value proplist=chain,action,protocol,dst-port,to-addresses,to-ports,comment,disabled]",
    "addresses": "[/ip/address/print as-value proplist=address,interface]",
    "wanStats": "[/interface/print stats as-value proplist=name,rx-byte,tx-byte where name=ether1]",
    "linkQuality": "[/interface/wireless/registration-table/print as-value proplist=mac-address,tx-ccq,p-throughput]",
    "radios": "[/interface/wireless/monitor [find] once as-value]",
    # newer boards use the `wifi` package instead; each router answers one of the two sets
    "wifiNew": "[/interface/wifi/print as-value proplist=name,channel.band,channel.width,configuration.ssid,running,disabled,master-interface,configuration.hide-ssid,datapath.client-isolation,security.authentication-types,comment]",
    "radiosNew": "[/interface/wifi/monitor [find] once as-value]",
    "stationsNew": "[/interface/wifi/registration-table/print as-value proplist=interface,mac-address,signal,tx-rate,rx-rate,uptime,bytes,last-ip,band,ssid]",
    # --- settings pages (explicit proplists: never keys, passphrases or script bodies) ---
    "wireguard": "[/interface/wireguard/print as-value proplist=name,listen-port,mtu,running,disabled,comment,public-key]",
    "wgPeers": "[/interface/wireguard/peers/print as-value proplist=interface,name,allowed-address,endpoint-address,endpoint-port,last-handshake,rx,tx,comment,disabled,public-key]",
    # no private-key: Back To Home users carry their WireGuard keys
    "bthUsers": "[/ip/cloud/back-to-home-users/print as-value proplist=name,allow-lan,disabled,comment,expires]",
    "routingRules": "[/routing/rule/print as-value proplist=src-address,dst-address,action,table,interface,routing-mark,disabled,comment]",
    "routingTables": "[/routing/table/print as-value proplist=name,fib,comment]",
    # read-only views; no BGP tcp-md5-key or OSPF auth keys
    "ospfInstances": "[/routing/ospf/instance/print as-value proplist=name,version,router-id,disabled]",
    "ospfNeighbors": "[/routing/ospf/neighbor/print as-value proplist=instance,address,router-id,state,adjacency]",
    "bgpSessions": "[/routing/bgp/session/print as-value proplist=name,remote.address,remote.as,local.as,established,uptime,prefix-count]",
    "routes": "[/ip/route/print as-value proplist=dst-address,gateway,distance,routing-table,active,dynamic,static,comment,disabled]",
    "dnsStatic": "[/ip/dns/static/print as-value proplist=name,address,type,comment,disabled]",
    "dnsCfg": '({"servers"=[/ip/dns/get servers];"use-doh-server"=[/ip/dns/get use-doh-server];"allow-remote-requests"=[/ip/dns/get allow-remote-requests];"mdns-repeat-ifaces"=[/ip/dns/get mdns-repeat-ifaces]})',
    "dnsCache": "[/ip/dns/cache/print as-value proplist=name,type,data,ttl]",
    "health": "[/system/health/print as-value proplist=name,value,type]",
    "queues": "[/queue/simple/print as-value proplist=name,target,max-limit,disabled,comment,time]",
    "hotspot": "[/ip/hotspot/print as-value proplist=name,interface,address-pool,profile,disabled]",
    "hotspotActive": "[/ip/hotspot/active/print as-value proplist=server,user,address,mac-address,login-by,uptime,session-time-left,bytes-in,bytes-out]",
    "users": "[/user/print as-value proplist=name,group,last-logged-in,disabled,comment]",
    "userGroups": "[/user/group/print as-value proplist=name,policy]",
    "scheduler": "[/system/scheduler/print as-value proplist=name,start-time,interval,disabled,next-run]",
    "clock": '({"time-zone-name"=[/system/clock/get time-zone-name];"date"=[/system/clock/get date];"time"=[/system/clock/get time]})',
    "ntp": '({"enabled"=[/system/ntp/client/get enabled];"servers"=[/system/ntp/client/get servers]})',
    "snmp": '({"enabled"=[/snmp/get enabled];"contact"=[/snmp/get contact];"location"=[/snmp/get location]})',
    "upnp": '({"enabled"=[/ip/upnp/get enabled]})',
    "pools": "[/ip/pool/print as-value proplist=name,ranges]",
    "dhcpServers": "[/ip/dhcp-server/print as-value proplist=name,interface,address-pool,lease-time,disabled]",
    "dhcpNetworks": "[/ip/dhcp-server/network/print as-value proplist=address,gateway,dns-server,domain,comment]",
    "vlans": "[/interface/vlan/print as-value proplist=name,vlan-id,interface,comment]",
    "bridges": "[/interface/bridge/print as-value proplist=name,vlan-filtering,protocol-mode,igmp-snooping,comment]",
    "bridgePorts": "[/interface/bridge/port/print as-value proplist=interface,bridge,pvid,frame-types,hw,disabled,edge,bpdu-guard]",
    "bonds": "[/interface/bonding/print as-value proplist=name,slaves,mode,running,disabled,comment]",
    "switches": "[/interface/ethernet/switch/print as-value proplist=name,type,mirror-source,mirror-target]",
    "switchPorts": "[/interface/ethernet/switch/port/print as-value proplist=name,storm-rate]",
    "dot1x": "[/interface/dot1x/server/print as-value proplist=interface,auth-types,disabled,comment]",
    # never the RADIUS secret
    "radius": "[/radius/print as-value proplist=service,disabled]",
    "discovery": "[/ip/neighbor/discovery-settings/get]",
    "bridgeVlans": "[/interface/bridge/vlan/print as-value proplist=bridge,vlan-ids,tagged,untagged]",
    "listMembers": "[/interface/list/member/print as-value proplist=list,interface]",
    "netwatch": "[/tool/netwatch/print as-value proplist=host,status,interval,type,comment]",
    "loggingRules": "[/system/logging/print as-value proplist=topics,action,disabled]",
    "neighbors": "[/ip/neighbor/print as-value proplist=interface,address,mac-address,identity,platform,board,version]",
    "secProfiles": "[/interface/wireless/security-profiles/print as-value proplist=name,mode,authentication-types,management-protection]",
    "accessList": "[/interface/wireless/access-list/print as-value proplist=mac-address,interface,signal-range,authentication,forwarding,comment]",
    "trafficFlow": '({"enabled"=[/ip/traffic-flow/get enabled]})',
    "ovpn": "[/interface/ovpn-server/server/print as-value]",
    "l2tp": '({"enabled"=[/interface/l2tp-server/server/get enabled]})',
    "backups": "[/file/print as-value proplist=name,type,size,last-modified where type=backup]",
    "exports": "[/file/print as-value proplist=name,type,size,last-modified where type=script]",
    "addressLists": "[/ip/firewall/address-list/print as-value proplist=list,address,dynamic,comment]",
    "etherStats": "[/interface/print stats as-value proplist=name,rx-byte,tx-byte,rx-error,tx-error,rx-drop,tx-drop where type=ether]",
    "etherMon": "[/interface/ethernet/monitor [find] once as-value]",
    "ipv6": '({"disable-ipv6"=[/ipv6/settings/get disable-ipv6]})',
    "ipv6Pools": "[/ipv6/pool/print as-value proplist=name,prefix,prefix-length]",
    "ipv6Addresses": "[/ipv6/address/print as-value proplist=address,interface,from-pool,advertise,dynamic,comment]",
    "kidControl": "[/ip/kid-control/print as-value proplist=name,rate-limit,disabled]",
    "services": "[/ip/service/print as-value proplist=name,port,disabled]",
    "log": "[/log/print as-value proplist=time,topics,message]",
}
# ponytail: one ssh call per query (~20 calls) so one bad query can't sink the rest
snap = {}
for key, expr in Q.items():
    r = subprocess.run(["ssh", "-o", "BatchMode=yes", HOST, f":put [:serialize to=json {expr}]"],
                       capture_output=True, text=True)
    body = r.stdout.strip()
    try:
        snap[key] = json.loads(body)
    except json.JSONDecodeError:
        snap[key] = body or r.stderr.strip()
        print(f"warn: {key}: {snap[key][:100]}", file=sys.stderr)
# Latency: :serialize truncates ping times to whole seconds, so parse the text summary instead
snap["latency"] = {}
for name, ip in [("Cloudflare", "1.1.1.1"), ("Google", "8.8.8.8"), ("Quad9", "9.9.9.9")]:
    out = subprocess.run(["ssh", "-o", "BatchMode=yes", HOST, f"/ping {ip} count=3"], capture_output=True, text=True).stdout
    m = re.search(r"avg-rtt=(?:(\d+)ms)?(?:(\d+)us)?", out)
    loss = re.search(r"packet-loss=(\d+)%", out)
    snap["latency"][name] = {"ms": round(int(m[1] or 0) + int(m[2] or 0) / 1000, 1) if m else None, "loss": int(loss[1]) if loss else None}
if isinstance(snap.get("etherMon"), list):
    snap["etherMon"] = [{k: r.get(k) for k in ("name", "status", "rate", "full-duplex", "auto-negotiation")} for r in snap["etherMon"]]
if isinstance(snap.get("ovpn"), list):
    snap["ovpn"] = [{k: r.get(k) for k in ("name", "disabled", "port", "protocol")} for r in snap["ovpn"]]
if isinstance(snap.get("dnsCache"), list):
    snap["dnsCache"] = [c for c in snap["dnsCache"] if c.get("type") in ("A", "AAAA")][:300]
# monitor output is huge; keep only what the UI shows
if isinstance(snap.get("radios"), list):
    keep = ("name", "channel", "noise-floor", "overall-tx-ccq", "registered-clients", "status")
    snap["radios"] = [{k: r.get(k) for k in keep} for r in snap["radios"]]
if isinstance(snap.get("radiosNew"), list):
    snap["radiosNew"] = [{k: r.get(k) for k in ("channel", "state", "registered-peers", "tx-power")} for r in snap["radiosNew"]]

if isinstance(snap.get("log"), list):
    # drop this script's own ssh logins, they'd drown the event list
    snap["log"] = [l for l in snap["log"] if "via ssh" not in l["message"] and "publickey accepted" not in l["message"]][-40:]
json.dump(snap, sys.stdout, indent=1)
