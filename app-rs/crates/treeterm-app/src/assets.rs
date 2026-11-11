//! Icons: GPUI Kit's own component icons (checkboxes, dialogs, menus…) plus the Lucide icons
//! the Electron UI draws through `lucide-react`, under their current Lucide names
//! (`Loader2` → `LoaderCircle`, `AlertTriangle` → `TriangleAlert`, `CheckCircle2` →
//! `CircleCheck`, `XCircle` → `CircleX`, `AlertCircle` → `CircleAlert`, `Filter` → `Funnel`).

use std::borrow::Cow;

use gpui_kit::{AssetSource, SharedString};

gpui_kit::assets::icon_assets!(
    LucideIcons,
    [
        ArrowDownToLine,
        Box,
        Check,
        ChevronDown,
        ChevronRight,
        ChevronUp,
        CircleAlert,
        CircleCheck,
        CircleDot,
        CircleX,
        Clock,
        Columns2,
        Download,
        ExternalLink,
        Eye,
        EyeOff,
        File,
        FileText,
        Folder,
        FolderOpen,
        Funnel,
        GitBranch,
        GitFork,
        Github,
        LoaderCircle,
        LockOpen,
        MessageSquare,
        Monitor,
        PanelLeftClose,
        PanelLeftOpen,
        Pencil,
        Plus,
        RefreshCw,
        RotateCcw,
        Search,
        Space,
        Square,
        Star,
        TextAlignJustify,
        TriangleAlert,
        Volume2,
        VolumeX,
        X,
    ]
);

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        match LucideIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(LucideIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
