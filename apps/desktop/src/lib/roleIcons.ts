import {
  Award, Brain, Code, Coffee, Crown, Feather, Flame, Gamepad2, Gem, Ghost, Heart, Leaf, Moon, Music, Palette, Rocket,
  Shield, Sparkles, Star, Sun, Swords, Trophy, Wand2, Zap, type LucideIcon,
} from "lucide-react";

export const ROLE_ICONS: Record<string, LucideIcon> = {
  crown: Crown, star: Star, sparkles: Sparkles, gem: Gem, trophy: Trophy, award: Award, shield: Shield, swords: Swords,
  zap: Zap, flame: Flame, rocket: Rocket, wand: Wand2, brain: Brain, code: Code, palette: Palette, feather: Feather,
  music: Music, gamepad: Gamepad2, heart: Heart, coffee: Coffee, leaf: Leaf, sun: Sun, moon: Moon, ghost: Ghost,
};

export function roleIcon(key: string): LucideIcon {
  return ROLE_ICONS[key] ?? Star;
}
