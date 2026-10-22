; c64.inc - the registers and colors of the C64, and a macro
SCREEN      = $0400
COLOR_RAM   = $d800
BORDER      = $d020
BACKGROUND  = $d021
RASTER      = $d012
VIC_MEMORY  = $d018

BLACK = 0
WHITE = 1
BLUE  = 6

; fill the screen with a char, the color RAM with a color
!macro clear_screen .char, .color {
        ldx #0
-       lda #.char
        sta SCREEN,x
        sta SCREEN + $100,x
        sta SCREEN + $200,x
        sta SCREEN + $2e8,x
        lda #.color
        sta COLOR_RAM,x
        sta COLOR_RAM + $100,x
        sta COLOR_RAM + $200,x
        sta COLOR_RAM + $2e8,x
        inx
        bne -
}
