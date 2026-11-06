# tikontrol installer for RouterOS 7.22+ (container package, /app).
# Install with one paste in a RouterOS terminal:
#   /tool/fetch url="https://raw.githubusercontent.com/MatejMa2ur/tikontrol/master/deploy/install.rsc" dst-path=tikontrol-install.rsc; /import tikontrol-install.rsc
#
# Safe to run again: it skips what is already done. It never formats a disk, never touches the plain
# `api` service, the firewall or any user. Everything it creates carries a comment starting with "tik:".
# Optional before running: `:global tikDisk "usb1"` to choose the disk yourself.
# Undo with deploy/uninstall.rsc. Docs: docs/INSTALL.md.
#
# The manual steps behind it were tested by hand on a hAP ac2 with 7.26beta1.
# This script itself has not been run on hardware yet.

# ---------------------------------------------------------------------------------------------
# Part 1: checks. Uses only commands every RouterOS 7 has, so it can explain what is missing.
# Each block below is parsed on its own: /app and /container only exist with the container package.
# ---------------------------------------------------------------------------------------------
{
:global tikDisk
:global tikInstDisk ""
:global tikInstOk false
:local stop false
:put "tikontrol: checking this router..."

# RouterOS version: /app needs 7.22 or later.
:local ver [/system/resource/get version]
:local dot [:find $ver "."]
:local major [:tonum [:pick $ver 0 $dot]]
:local minor 0
:local i ($dot + 1)
:while (($i < [:len $ver]) && ([:typeof [:tonum [:pick $ver $i]]] = "num")) do={
  :set minor ($minor * 10 + [:tonum [:pick $ver $i]])
  :set i ($i + 1)
}
:if (($major < 7) || (($major = 7) && ($minor < 22))) do={
  :put "  [stop] RouterOS $ver is too old. tikontrol on the router needs 7.22 or later (System > Packages > Check For Updates)."
  :set stop true
} else={ :put "  [ok] RouterOS $ver" }

# CPU architecture: the image is built for arm (v7), arm64 and x86_64.
:local arch [/system/resource/get architecture-name]
:if (($arch = "arm") || ($arch = "arm64") || ($arch = "x86_64") || ($arch = "x86")) do={
  :put "  [ok] architecture $arch"
  :if (($arch = "arm") && ($major = 7) && ($minor < 26)) do={
    :put "  [warn] Apps on 32-bit ARM arrived with 7.26. If adding the app fails below, upgrade RouterOS."
  }
} else={
  :put "  [stop] This router's CPU ($arch) cannot run containers or has no tikontrol build."
  :put "         Run tikontrol on a home server instead, see docs/DEPLOY.md (Docker)."
  :set stop true
}

# Free RAM: tikontrol needs a few MB, the container runtime more.
:local freeMem ([/system/resource/get free-memory] / 1048576)
:if ($freeMem < 20) do={
  :put "  [warn] Only $freeMem MB RAM free. tikontrol may be killed when memory runs out."
} else={ :put "  [ok] $freeMem MB RAM free" }

# Container package: /container and /app only exist when it is installed.
:local hasContainer true
:do { :local p [:parse "/container/config/print"] } on-error={ :set hasContainer false }
:do { :local p [:parse "/app/print"] } on-error={ :set hasContainer false }
:if ($hasContainer) do={ :put "  [ok] container package installed" } else={
  :put "  [stop] The container package is not installed."
  :put "         Download the 'Extra packages' zip for your architecture and RouterOS version from"
  :put "         https://mikrotik.com/download, upload container-*.npk to Files, then reboot:"
  :put "         /system/reboot"
  :set stop true
}

# Device mode: containers must be allowed. Only a person at the router can confirm this.
:local dm [:tostr [/system/device-mode/get container]]
:if (($dm = "true") || ($dm = "yes")) do={ :put "  [ok] device-mode allows containers" } else={
  :put "  [stop] Containers are switched off in device-mode. Run this, then within 5 minutes press the"
  :put "         reset/mode button briefly or unplug the power (a soft reboot cancels it):"
  :put "         /system/device-mode/update container=yes"
  :put "         Then run the installer again."
  :set stop true
}

# Storage: apps need an ext4 disk. Internal flash is too small on most routers.
:local disk ""
:local unusable ""
:foreach d in=[/disk/find] do={
  :local slot [/disk/get $d slot]
  :local fs ""
  :do { :set fs [:tostr [/disk/get $d fs]] } on-error={}
  :if ($fs = "ext4") do={
    :if (($disk = "") || ($slot = $tikDisk)) do={ :set disk $slot }
  } else={
    :if ($slot != "") do={ :set unusable ($unusable . " " . $slot) }
  }
}
:local freeHdd ([/system/resource/get free-hdd-space] / 1048576)
:if ($disk != "") do={ :put "  [ok] using disk $disk (ext4)" } else={
  :if ([:len $unusable] > 0) do={
    :put "  [stop] Found disk(s)$unusable, but none is formatted ext4 (FAT32 does not work for containers)."
    :put "         To use one, format it as ext4. WARNING: this ERASES EVERYTHING on that disk. Copy off"
    :put "         anything you need first, check the disk name with /disk/print, then run (example):"
    :put "         /disk/format usb1 file-system=ext4 label=tik"
    :put "         Then run the installer again."
    :set stop true
  } else={
    :if ($freeHdd >= 100) do={
      :put "  [ok] no external disk; using internal storage ($freeHdd MB free)"
    } else={
      :put "  [stop] No ext4 disk found and only $freeHdd MB internal storage free."
      :put "         Plug in a USB stick or microSD card (1 GB is plenty), format it as ext4 (see docs/INSTALL.md),"
      :put "         then run the installer again."
      :set stop true
    }
  }
}

:if ($stop) do={
  :put ""
  :put "tikontrol: nothing was changed. Fix the [stop] items above and run the installer again."
  :error "tikontrol: stopped by a pre-check"
}
:set tikInstDisk $disk
:set tikInstOk true
}

# ---------------------------------------------------------------------------------------------
# Part 2: API-SSL, so tikontrol can read the router over TLS. Plain `api` is left as it is.
# ---------------------------------------------------------------------------------------------
{
:global tikInstOk
:if ($tikInstOk != true) do={ :error "tikontrol: pre-checks did not pass" }

# LAN: the first bridge with an address, preferring one in the LAN interface list.
:global tikInstBridge ""
:global tikInstLan ""
:local pickedInLan false
:foreach a in=[/ip/address/find where disabled=no] do={
  :local ifc [/ip/address/get $a interface]
  :if ([:len [/interface/bridge/find where name=$ifc]] > 0) do={
    :local inLan ([:len [/interface/list/member/find where list="LAN" and interface=$ifc]] > 0)
    :if (($tikInstBridge = "") || ($inLan && !$pickedInLan)) do={
      :local addr [/ip/address/get $a address]
      :set tikInstBridge $ifc
      :set tikInstLan ([/ip/address/get $a network] . [:pick $addr [:find $addr "/"] [:len $addr]])
      :set pickedInLan $inLan
    }
  }
}
:if ($tikInstBridge = "") do={
  :put "  [stop] Found no bridge with an IP address (the LAN). tikontrol joins the LAN bridge."
  :error "tikontrol: no LAN bridge"
}
:put "  [ok] LAN is $tikInstBridge ($tikInstLan)"

# Certificate: self-signed. key-cert-sign is needed, 7.26 refuses to sign without it ("CA not found").
:if ([:len [/certificate/find where name="tikontrol-api"]] = 0) do={
  :put "tikontrol: creating certificate tikontrol-api (takes a few seconds)..."
  /certificate/add name=tikontrol-api common-name=router key-usage=tls-server,key-cert-sign days-valid=3650 comment="tik: tikontrol api-ssl"
}
:local cert [/certificate/find where name="tikontrol-api"]
:local fp ""
:do { :set fp [/certificate/get $cert fingerprint] } on-error={}
:if ([:len $fp] = 0) do={
  /certificate/sign $cert
  :local n 0
  :while (([:len $fp] = 0) && ($n < 30)) do={
    :delay 2s
    :do { :set fp [/certificate/get $cert fingerprint] } on-error={}
    :set n ($n + 1)
  }
}
:if ([:len $fp] = 0) do={ :error "tikontrol: signing tikontrol-api did not finish. Check /certificate/print and run again." }

# api-ssl: keep a certificate or address limit someone already set; otherwise use ours and the LAN.
:local svc [/ip/service/find where name="api-ssl"]
:local curCert [:tostr [/ip/service/get $svc certificate]]
:if (($curCert = "") || ($curCert = "none")) do={
  :local f [:parse "/ip/service/set [find where name=\"api-ssl\"] certificate=tikontrol-api tls-version=only-1.2"]
  $f
  :put "  [ok] api-ssl uses certificate tikontrol-api"
} else={ :put "  [ok] api-ssl keeps its certificate $curCert" }
# 7.24 renamed `address` to `available-from`: feature-detect.
:local newName true
:local from ""
:do { :set from [:tostr [/ip/service/get $svc available-from]] } on-error={
  :set newName false
  :set from [:tostr [/ip/service/get $svc address]]
}
:if (($from = "") || ($from = "0.0.0.0/0")) do={
  :local f
  :if ($newName) do={ :set f [:parse "/ip/service/set [find where name=\"api-ssl\"] available-from=\$net"] } else={
    :set f [:parse "/ip/service/set [find where name=\"api-ssl\"] address=\$net"]
  }
  $f net=$tikInstLan
  :put "  [ok] api-ssl limited to $tikInstLan"
} else={ :put "  [ok] api-ssl keeps its address limit ($from); make sure it includes the tikontrol app's IP" }
/ip/service/set $svc disabled=no
}

# ---------------------------------------------------------------------------------------------
# Part 3: the app itself.
# ---------------------------------------------------------------------------------------------
{
:global tikInstOk
:global tikInstDisk
:global tikInstBridge
:if ($tikInstOk != true) do={ :error "tikontrol: pre-checks did not pass" }

# The app YAML, kept identical to deploy/routeros-app.yaml (tests/install.test.ts checks this).
:local yaml ""
# BEGIN app yaml
:set yaml ($yaml . "name: tikontrol\n")
:set yaml ($yaml . "descr: 'Read-only-by-default web UI for this router'\n")
:set yaml ($yaml . "page: 'https://github.com/MatejMa2ur/tikontrol'\n")
:set yaml ($yaml . "category: networking\n")
:set yaml ($yaml . "'default-credentials': 'setup token: see the container log'\n")
:set yaml ($yaml . "services:\n")
:set yaml ($yaml . "  tikontrol:\n")
:set yaml ($yaml . "    image: 'ghcr.io/matejma2ur/tikontrol:latest'\n")
:set yaml ($yaml . "    environment:\n")
:set yaml ($yaml . "      - 'TIK_DATA=/data'\n")
:set yaml ($yaml . "      - 'TIK_LISTEN=0.0.0.0:8443'\n")
:set yaml ($yaml . "    ports:\n")
:set yaml ($yaml . "      - '8443:8443:web'\n")
:set yaml ($yaml . "    restart: 'unless-stopped'\n")
:set yaml ($yaml . "    volumes:\n")
:set yaml ($yaml . "      - 'data:/data'\n")
:set yaml ($yaml . "'supported-archs':\n")
:set yaml ($yaml . "  - arm\n")
:set yaml ($yaml . "  - 'arm64'\n")
:set yaml ($yaml . "  - 'x86'\n")
# END app yaml

:local app [/app/find where name="tikontrol"]
:if ([:len $app] = 0) do={
  :if ($tikInstDisk != "") do={
    /app/settings/set disk=$tikInstDisk
    # A copy for reference or a manual /app/add; not needed by the app.
    :do { /file/add name="$tikInstDisk/tikontrol-app.yaml" contents=$yaml } on-error={}
  }
  :put "tikontrol: adding the app on $tikInstBridge..."
  # 7.21+ takes a bridge name as network; fall back to the generic LAN setting.
  :do { /app/add yaml=$yaml network=$tikInstBridge auto-update=yes } on-error={
    /app/add yaml=$yaml network=lan auto-update=yes
  }
  :set app [/app/find where name="tikontrol"]
} else={
  # Never re-set yaml on an existing app: RouterOS 7.23+ then cleans up the app, data included.
  :put "tikontrol: app already installed, checking its settings (update it with /app/update tikontrol)"
}
/app/set $app auto-update=yes
# use-https=yes waits for a MikroTik-cloud certificate; tikontrol serves its own HTTPS on 8443.
/app/set $app use-https=no

# Memory: apps ignore a per-container cap, so set the global one unless someone already did.
:local curMax ""
:do { :set curMax [:tostr [/container/config/get memory-max]] } on-error={ :set curMax "n/a" }
:if (($curMax = "") || ($curMax = "unlimited")) do={
  :do {
    :local f [:parse "/container/config/set memory-high=40M memory-max=48M"]
    $f
    :put "  [ok] container memory limited to 48 MB (global /container/config, applies to all containers)"
  } on-error={ :put "  [warn] could not set /container/config memory-max on this version" }
} else={ :put "  [ok] container memory limit already set ($curMax), left as it is" }

/app/enable $app
:put "tikontrol: starting; downloading the image can take a few minutes..."
:local ip ""
:local n 0
:while (([:len $ip] = 0) && ($n < 60)) do={
  :delay 5s
  :do { :set ip [:tostr [/app/get $app ip-address]] } on-error={}
  :set n ($n + 1)
}
:if ([:find $ip "/"] > 0) do={ :set ip [:pick $ip 0 [:find $ip "/"]] }
:if ([:len $ip] = 0) do={
  :put "  [warn] The app has no IP yet. Check /app/print detail where name=tikontrol, then run the installer again."
  :error "tikontrol: app not running yet"
}
:put "  [ok] app is at $ip"

# Pin the app's address: make its DHCP lease static.
:local lease [/ip/dhcp-server/lease/find where address=$ip]
:if ([:len $lease] > 0) do={
  :if ([/ip/dhcp-server/lease/get $lease dynamic]) do={
    /ip/dhcp-server/lease/make-static $lease
    /ip/dhcp-server/lease/set $lease comment="tik: tikontrol app"
    :put "  [ok] DHCP lease for $ip made static"
  }
} else={ :put "  [warn] no DHCP lease found for $ip; its address may change after a restart" }

# A name for it: tikontrol.lan (also in tikontrol's own certificate).
:local dns [/ip/dns/static/find where name="tikontrol.lan"]
:if ([:len $dns] = 0) do={
  /ip/dns/static/add name=tikontrol.lan address=$ip comment="tik: tikontrol app"
  :put "  [ok] DNS name tikontrol.lan -> $ip"
} else={
  :if ([/ip/dns/static/get $dns comment] ~ "^tik:") do={
    /ip/dns/static/set $dns address=$ip
  } else={ :put "  [warn] tikontrol.lan already exists in /ip/dns/static and is not ours; left as it is" }
}
:if (![/ip/dns/get allow-remote-requests]) do={
  :put "  [warn] the router's DNS does not answer the LAN (allow-remote-requests=no), so use the IP address"
}

:local certName [/ip/service/get [find where name="api-ssl"] certificate]
:local fp ""
:do { :set fp [/certificate/get [find where name=$certName] fingerprint] } on-error={}
:put ""
:put "tikontrol is installed."
:put "  Open https://tikontrol.lan:8443 (or https://$ip:8443). WebFig also lists it under App > tikontrol."
:put "  Router address for the setup page: the router's LAN IP, port 8729 (API-SSL)."
:put "  API-SSL certificate fingerprint (compare it on the setup page): $fp"
:put "  Setup token, if the page asks for one:"
:put "    /container/log/print where message~\"setup token\""
:put "  Updates install automatically (auto-update=yes); now: /app/update tikontrol"
}

# Clean up the installer's own variables (not tikDisk, which the user may have set).
/system/script/environment/remove [find where name~"^tikInst"]
