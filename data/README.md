# Sky-chart data

Two plain-text tables, compiled into the binary with `include_str!`.

## `stars.csv`

`ra,dec,mag,bv,name` — 9,096 stars, one per line, brightest first.
Right ascension and declination are J2000 degrees, `mag` is visual
magnitude, `bv` the B−V colour index (0.00 where the catalogue has none),
`name` the IAU proper name where the star has one (333 of them), else
empty.

Built from the **Yale Bright Star Catalogue, 5th Revised Edition**
(Hoffleit & Warren 1991), distributed by the Astronomical Data Center.
Public domain. <http://tdc-www.harvard.edu/catalogs/bsc5.html>

Proper names come from the **IAU Catalog of Star Names** (IAU Working
Group on Star Names), matched on HR number.
<https://www.pas.rochester.edu/~emamajek/WGSN/IAU-CSN.txt>

## `constellations.csv`

`ABR:ra,dec ra,dec …` — one polyline per line, 150 of them, in J2000
degrees. Several lines per constellation, since the stick figures branch.

From **d3-celestial** by Olaf Frohn, BSD 3-clause:
<https://github.com/ofrohn/d3-celestial>

    Copyright (c) 2015-2020, Olaf Frohn
    All rights reserved.
    Redistribution and use in source and binary forms, with or without
    modification, are permitted provided that the conditions of the
    BSD 3-clause licence are met.

Everything else in astro is public domain (Unlicense); this one file
carries Olaf Frohn's notice with it.
