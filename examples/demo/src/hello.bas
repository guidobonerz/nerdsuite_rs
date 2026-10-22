10 rem nerdsuite demo in basic v2
20 print "{clr}{wht}hello from {yel}nerdsuite{wht}!"
30 for i=0 to 15
40 poke 53280,i:poke 53281,15-i
50 for d=1 to 100:next d
60 next i
70 goto 30
