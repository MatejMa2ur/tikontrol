# What the routers look like (for our own drawing)

Notes behind `faceOf()` in `src/ui/src/lib/hardware.ts` and `src/ui/src/lib/DeviceFace.svelte`. Text sources only:
no images, logos or artwork were copied. The drawing is original and simplified.

## hAP ac² (RBD52G-5HacD2HnD)

- Case: light/white plastic "universal" case, used flat (desktop) or upright (tower) on the included base; wall-mount kit. 34 × 119 × 98 mm, passive cooling.
- Ports: 5 × 10/100/1000 Ethernet. Port 1 is the Internet port and takes passive PoE in (18–28 V).
- Power: DC barrel jack 5.5/2 mm, 12–30 V, or PoE on port 1. Max 21 W.
- USB: 1 × USB type A (1 A, USB power reset).
- Buttons/LEDs: a reset (RouterBOOT) button; the manual only describes a green LED that flashes during reset. Forum users note per-Ethernet-port LEDs and no wireless LED on the tower-case hAP models.
- Not confirmed from text: exact left/right order of jack, USB and reset on the panel. We draw power, reset and USB to the left of ports 1→5.

## hAP be³ Media (MA53UG+HbeH, `?device=be3`)

- Ports: 5 × 2.5G Ethernet, port 1 with PoE in (802.3af/at).
- Power: DC jack 24–57 V (24 V 1.5 A adapter included) or PoE in. Max 29 W, passive cooling, case base and wall-mount guide.
- USB: 1 × USB-C and 2 × USB-A 3.0 (1.5 A), plus a microSD slot. Internal antennas.
- Not confirmed from text: case colour (drawn light like the rest of the hAP family), LED layout, panel order.

## Generic fallback

Any other model gets a neutral grey rounded box with the Ethernet ports the router reports, power jack, reset and
power LED, and no USB or speed label (we don't guess hardware we can't detect). Add a row to `FACES` once a model's
spec page is checked.

## Sources

- MikroTik, hAP ac² product page: https://mikrotik.com/product/hap_ac2
- MikroTik, hAP ac² user manual: https://help.mikrotik.com/docs/x/MYH5
- MikroTik forum, LEDs on the vertical-case hAP ac: https://forum.mikrotik.com/t/led-wifi-hap-ac-vertical-case/145031
- MikroTik, hAP be³ Media product page: https://mikrotik.com/product/hap_be3_media
- Reseller spec listings for the be³ Media ports (USB-C, 2 × USB-A, microSD): https://linitx.com/product/MikroTik-hAP-be3-Media-WiFi-7-Router-Access-Point-MA53UG+HbeH/18454, https://interprojekt.pl/en/p/mikrotik-ma53ug-hbeh.html
