# zmk-sofleii-id — a fixed identity record in each half's flash image, so
# sofleii can tell which keyboard and half a UF2 bootloader drive holds by
# reading its CURRENT.UF2 before flashing (#98). Both halves build it.
if(CONFIG_SOFLEII_ID)
    target_sources(app PRIVATE src/sofleii_id.c)
endif()
