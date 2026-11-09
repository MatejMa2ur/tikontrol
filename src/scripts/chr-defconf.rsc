# Home-router style config for the test CHR, modelled on MikroTik's default configuration.
# Fictional/default values only. ether4 and ether5 stay out of the bridge (spare ports for bond and WAN tests). Paste over SSH: src/scripts/chr.sh forwards ssh to localhost:2222.
/interface bridge add name=bridge comment=defconf
/interface bridge port add bridge=bridge interface=ether2 comment=defconf
/interface bridge port add bridge=bridge interface=ether3 comment=defconf
/interface list add name=WAN comment=defconf
/interface list add name=LAN comment=defconf
/interface list member add list=LAN interface=bridge comment=defconf
/interface list member add list=WAN interface=ether1 comment=defconf
/ip address add address=192.168.88.1/24 interface=bridge comment=defconf
/ip pool add name=default-dhcp ranges=192.168.88.10-192.168.88.254
/ip dhcp-server add name=defconf address-pool=default-dhcp interface=bridge
/ip dhcp-server network add address=192.168.88.0/24 gateway=192.168.88.1 dns-server=192.168.88.1 comment=defconf
/ip dns set allow-remote-requests=yes
/ip dns static add name=router.lan address=192.168.88.1 type=A comment=defconf
/ip firewall nat add chain=srcnat action=masquerade out-interface-list=WAN ipsec-policy=out,none comment="defconf: masquerade"
/ip firewall filter add chain=input action=accept connection-state=established,related,untracked comment="defconf: accept established,related,untracked"
/ip firewall filter add chain=input action=drop connection-state=invalid comment="defconf: drop invalid"
/ip firewall filter add chain=input action=accept protocol=icmp comment="defconf: accept ICMP"
/ip firewall filter add chain=input action=accept dst-address=127.0.0.1 comment="defconf: accept to local loopback (for CAPsMAN)"
/ip firewall filter add chain=input action=accept in-interface=ether1 protocol=tcp dst-port=22,8728,8729 comment="test: management from the host through QEMU NAT"
/ip firewall filter add chain=input action=drop in-interface-list=!LAN comment="defconf: drop all not coming from LAN"
/ip firewall filter add chain=forward action=accept ipsec-policy=in,ipsec comment="defconf: accept in ipsec policy"
/ip firewall filter add chain=forward action=accept ipsec-policy=out,ipsec comment="defconf: accept out ipsec policy"
/ip firewall filter add chain=forward action=fasttrack-connection connection-state=established,related comment="defconf: fasttrack"
/ip firewall filter add chain=forward action=accept connection-state=established,related,untracked comment="defconf: accept established,related, untracked"
/ip firewall filter add chain=forward action=drop connection-state=invalid comment="defconf: drop invalid"
/ip firewall filter add chain=forward action=drop connection-state=new connection-nat-state=!dstnat in-interface-list=WAN comment="defconf: drop all from WAN not DSTNATed"
/ip neighbor discovery-settings set discover-interface-list=LAN
/tool mac-server set allowed-interface-list=LAN
/tool mac-server mac-winbox set allowed-interface-list=LAN
