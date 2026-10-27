#!/bin/sh
getent group solidway >/dev/null || groupadd -r solidway
getent passwd solidway >/dev/null || useradd -r -g solidway -d /var/lib/solidway -s /sbin/nologin -c "Solidway service" solidway
install -d -o solidway -g solidway -m 0750 /var/lib/solidway
exit 0
