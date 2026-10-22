# Java certificate distrust update

`blocked.certs` is the standard OpenJDK vendor list from
<https://ops.java/unix/blocked.certs> (2026-10-22), used according to
<https://openjdk.org/groups/vulnerability/advisories/2026-10-21>.
CI installs this vendor data into its disposable JDK. Local Iris tasks require
a vendor-updated JDK, or the vendor-documented update applied to your own JDK;
they do not change shared tool installations.
