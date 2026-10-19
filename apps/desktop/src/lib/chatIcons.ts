import {
  BookOpen, Bot, Briefcase, Camera, Code, Coffee, FlaskConical, Gamepad2, Globe, GraduationCap, Hash, Heart, Leaf,
  Lightbulb, MessageCircle, Music, Palette, PenTool, Rocket, Shield, Star, Terminal, Wrench, Zap, type LucideIcon,
} from "lucide-react";

export const CHAT_ICONS: Record<string, LucideIcon> = {
  hash: Hash, code: Code, "message-circle": MessageCircle, rocket: Rocket, "graduation-cap": GraduationCap, palette: Palette,
  briefcase: Briefcase, "flask-conical": FlaskConical, "book-open": BookOpen, music: Music, camera: Camera, globe: Globe,
  heart: Heart, lightbulb: Lightbulb, shield: Shield, terminal: Terminal, wrench: Wrench, "pen-tool": PenTool,
  coffee: Coffee, star: Star, zap: Zap, leaf: Leaf, "gamepad-2": Gamepad2, bot: Bot,
};

export function chatIcon(name: string): LucideIcon {
  return CHAT_ICONS[name] ?? Hash;
}
