# tikontrol uninstaller: undoes deploy/install.rsc.
#   /tool/fetch url="https://raw.githubusercontent.com/MatejMa2ur/tikontrol/master/deploy/uninstall.rsc" dst-path=tikontrol-uninstall.rsc; /import tikontrol-uninstall.rsc
#
# By default tikontrol's data (settings, encryption key, history) stays on the disk and the app is only
# disabled, so it can be enabled again. To delete the app AND its data for good, first run:
#   :global keepData "no"
# Removes only objects with a "tik:" comment. Never touches api-ssl's settings, users, the firewall or disks.
# Untested on hardware.

# The DHCP lease, DNS name and certificate (these menus exist on every RouterOS 7).
{
:foreach l in=[/ip/dhcp-server/lease/find where comment~"^tik:"] do={ /ip/dhcp-server/lease/remove $l }
:foreach d in=[/ip/dns/static/find where comment~"^tik:"] do={ /ip/dns/static/remove $d }
:put "tikontrol: removed the static DHCP lease and the tikontrol.lan DNS name"
:local cert [/certificate/find where name="tikontrol-api" and comment~"^tik:"]
:if ([:len $cert] > 0) do={
  :if ([:len [/ip/service/find where certificate="tikontrol-api"]] = 0) do={
    /certificate/remove $cert
    :put "tikontrol: removed certificate tikontrol-api"
  } else={
    :put "tikontrol: certificate tikontrol-api is still used by api-ssl, so it stays. If nothing else uses"
    :put "  API-SSL, you can turn it off and then remove the certificate:"
    :put "  /ip/service/set [find name=api-ssl] disabled=yes certificate=none"
    :put "  /certificate/remove [find name=tikontrol-api]"
  }
}
}

# The app (only with the container package; this block fails harmlessly without it).
{
:global keepData
:local app [/app/find where name="tikontrol"]
:if ([:len $app] > 0) do={
  /app/disable $app
  :if ($keepData = "no") do={
    :local disk [/app/settings/get disk]
    /app/remove $app
    :local dir "apps/tikontrol"
    :if ($disk != "") do={ :set dir "$disk/apps/tikontrol" }
    :do { /file/remove [find where name=$dir] } on-error={ :put "tikontrol: could not remove $dir, remove it in Files" }
    :put "tikontrol: app and its data removed"
  } else={
    :put "tikontrol: app disabled, data kept. Enable again: /app/enable tikontrol"
    :put "  To delete it with its data: :global keepData \"no\" and run this uninstaller again."
  }
}
:put "tikontrol: done. /container/config memory limits were left as they are."
}
