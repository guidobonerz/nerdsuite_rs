; main.asm - the nerdsuite demo: a charset of the Chars editor, a text and raster bars
; Build: Ctrl+B · Run in VICE: F5 · Debug: F6 - click a line number for a breakpoint
!cpu 6510
!to "demo.prg", cbm
!source "c64.inc"

CHARSET = $3000                         ; where the charset goes (VIC bank 0)

*= $0801
        ; 10 SYS 2064
        !word basic_end, 10
        !byte $9e
        !text "2064"
        !byte 0
basic_end:
        !word 0

*= $0810
!zone main
start:
        sei
        lda #BLACK
        sta BORDER
        sta BACKGROUND
        +clear_screen ' ', WHITE
        ldx #0
.copy:  lda text,x                      ; the text into the middle of the screen
        beq .done
        sta SCREEN + 12 * 40 + 10,x
        inx
        bne .copy
.done:
        lda #(CHARSET / $0400) & $0e | $10 ; the screen at $0400, the charset at $3000
        sta VIC_MEMORY

!zone raster
.frame: ldy #0
.wait:  lda RASTER                      ; wait for the top of the bars
        cmp #$60
        bne .wait
.bar:   lda colors,y
        ldx RASTER
-       cpx RASTER                      ; one raster line
        beq -
        sta BORDER
        iny
        cpy #colors_end - colors
        bne .bar
        lda #BLACK
        sta BORDER
        jmp .frame

text:   !scr "hello from nerdsuite", 0
colors: !byte 6, 14, 3, 1, 3, 14, 6, 0, 9, 8, 7, 1, 7, 8, 9, 0
colors_end:

*= CHARSET
        !binary "../gfx/charset/chars.chr"
