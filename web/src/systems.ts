// Maps console names as they usually appear in ROM collections to EmuDeck's
// `roms/<system>` folder names, so a copy can land in the right place without
// the user hunting for it. Longer / more specific names come first.

const SYSTEMS: Array<[RegExp, string]> = [
  [/super nintendo|super famicom|\bsnes\b|\bsfc\b/, 'snes'],
  [/nintendo entertainment system|\bfamicom\b|\bnes\b/, 'nes'],
  [/nintendo 64|\bn64\b/, 'n64'],
  [/nintendo 3ds|\b3ds\b/, 'n3ds'],
  [/nintendo ds|\bnds\b/, 'nds'],
  [/game ?boy advance|\bgba\b/, 'gba'],
  [/game ?boy color|\bgbc\b/, 'gbc'],
  [/game ?boy|\bgb\b/, 'gb'],
  [/gamecube|game cube|\bngc\b/, 'gc'],
  [/wii ?u\b/, 'wiiu'],
  [/\bwii\b/, 'wii'],
  [/\bswitch\b/, 'switch'],
  [/virtual boy/, 'virtualboy'],
  [/playstation vita|ps ?vita/, 'psvita'],
  [/playstation portable|\bpsp\b/, 'psp'],
  [/playstation 3|\bps3\b/, 'ps3'],
  [/playstation 2|\bps2\b/, 'ps2'],
  [/playstation( 1| one)?\b|\bpsx\b|\bps1\b/, 'psx'],
  [/sega cd|mega[- ]?cd/, 'segacd'],
  [/32x/, 'sega32x'],
  [/mega ?drive|genesis/, 'genesis'],
  [/master system|mark iii/, 'mastersystem'],
  [/game gear/, 'gamegear'],
  [/saturn/, 'saturn'],
  [/dreamcast/, 'dreamcast'],
  [/sg-?1000/, 'sg-1000'],
  [/neo ?geo cd/, 'neogeocd'],
  [/neo ?geo pocket color/, 'ngpc'],
  [/neo ?geo pocket/, 'ngp'],
  [/neo ?geo/, 'neogeo'],
  [/xbox 360/, 'xbox360'],
  [/\bxbox\b/, 'xbox'],
  [/atari 2600/, 'atari2600'],
  [/atari 5200/, 'atari5200'],
  [/atari 7800/, 'atari7800'],
  [/atari lynx|\blynx\b/, 'atarilynx'],
  [/jaguar/, 'atarijaguar'],
  [/pc ?engine cd|turbografx[- ]?cd/, 'pcenginecd'],
  [/pc ?engine|turbografx/, 'pcengine'],
  [/wonderswan color/, 'wonderswancolor'],
  [/wonderswan/, 'wonderswan'],
  [/colecovision/, 'colecovision'],
  [/intellivision/, 'intellivision'],
  [/\bmsx\b/, 'msx'],
  [/commodore 64|\bc64\b/, 'c64'],
  [/amiga/, 'amiga'],
  [/\bdos\b|ms-dos/, 'dos'],
  [/\bmame\b|arcade|\bfba\b|final burn/, 'arcade'],
  [/\b3do\b/, '3do'],
]

/** Best EmuDeck system folder for a remote path, if any segment names a console. */
export function guessSystem(remotePath: string): string | null {
  const segments = remotePath.toLowerCase().split('/').reverse()
  for (const seg of segments) {
    for (const [re, folder] of SYSTEMS) {
      if (re.test(seg)) return folder
    }
  }
  return null
}
