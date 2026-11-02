// A small stroke icon set (24-unit grid), drawn for CalcBridge.
import type { ReactNode, SVGProps } from "react";

function make(paths: ReactNode) {
  return function Icon(props: SVGProps<SVGSVGElement>) {
    return (
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth={1.8}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
        {...props}
      >
        {paths}
      </svg>
    );
  };
}

export const IconFolder = make(<path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2.5h7.5A2.5 2.5 0 0 1 21 10v7.5a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5z" fill="currentColor" fillOpacity={0.18} />);
export const IconFolderPlus = make(<><path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2.5h7.5A2.5 2.5 0 0 1 21 10v7.5a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5z" /><path d="M12 11v6M9 14h6" /></>);
export const IconFile = make(<><path d="M6 3h8l4 4v14H6z" /><path d="M14 3v4h4" /></>);
export const IconDoc = make(<><path d="M6 3h8l4 4v14H6z" fill="currentColor" fillOpacity={0.12} /><path d="M14 3v4h4M9 12h6M9 15.5h6M9 9h2" /></>);
export const IconCode = make(<><path d="M6 3h8l4 4v14H6z" /><path d="M10 11l-2 2.5 2 2.5M14 11l2 2.5-2 2.5" /></>);
export const IconHome = make(<><path d="M4 11l8-7 8 7" /><path d="M6 9.5V20h12V9.5" /></>);
export const IconRefresh = make(<><path d="M20 12a8 8 0 1 1-2.4-5.7" /><path d="M20 4v5h-5" /></>);
export const IconUp = make(<><path d="M12 19V5M6 11l6-6 6 6" /></>);
export const IconSend = make(<><path d="M5 12h14M13 6l6 6-6 6" /></>);
export const IconReceive = make(<><path d="M19 12H5M11 6l-6 6 6 6" /></>);
export const IconPencil = make(<><path d="M4 20h4L19 9l-4-4L4 16z" /><path d="M13.5 6.5l4 4" /></>);
export const IconTrash = make(<><path d="M4 7h16M10 7V4h4v3M6 7l1 13h10l1-13" /><path d="M10 11v5M14 11v5" /></>);
export const IconScreen = make(<><rect x="3" y="4" width="18" height="13" rx="2" /><path d="M8 21h8M12 17v4" /></>);
export const IconKeypad = make(<><rect x="5" y="2.5" width="14" height="19" rx="3" /><rect x="8" y="5.5" width="8" height="5" rx="1" /><path d="M8.5 14h.01M12 14h.01M15.5 14h.01M8.5 17.5h.01M12 17.5h.01M15.5 17.5h.01" strokeWidth={2.6} /></>);
export const IconCreate = make(<><path d="M12 20h8" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" /></>);
export const IconBook = make(<><path d="M4 5.5A2.5 2.5 0 0 1 6.5 3H20v15H6.5A2.5 2.5 0 0 0 4 20.5z" /><path d="M4 20.5A2.5 2.5 0 0 0 6.5 23H20v-5" /></>);
export const IconList = make(<><path d="M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01" /></>);
export const IconGear = make(<><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" /></>);
export const IconPlug = make(<><path d="M9 2v5M15 2v5M7 7h10v4a5 5 0 0 1-10 0z" /><path d="M12 16v6" /></>);
export const IconX = make(<path d="M6 6l12 12M18 6L6 18" />);
export const IconCheck = make(<path d="M5 12.5l4.5 4.5L19 7.5" />);
export const IconPlay = make(<path d="M7 4.5v15l12-7.5z" fill="currentColor" fillOpacity={0.2} />);
export const IconPause = make(<path d="M8 5v14M16 5v14" />);
export const IconCamera = make(<><path d="M4 8h3l2-3h6l2 3h3v11H4z" /><circle cx="12" cy="13" r="3.5" /></>);
export const IconCopy = make(<><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3" /></>);
export const IconSave = make(<><path d="M5 3h11l3 3v15H5z" /><path d="M8 3v5h7V3M8 21v-7h8v7" /></>);
export const IconReveal = make(<><path d="M14 4h6v6M20 4l-8 8" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></>);
export const IconInfo = make(<><circle cx="12" cy="12" r="9" /><path d="M12 11v6M12 7.5h.01" /></>);
export const IconAlert = make(<><path d="M12 3l9.5 17h-19z" /><path d="M12 10v4.5M12 17.5h.01" /></>);
export const IconArchive = make(<><rect x="3" y="4" width="18" height="5" rx="1" /><path d="M5 9v11h14V9M10 13h4" /></>);
export const IconRestore = make(<><path d="M3 12a9 9 0 1 0 3-6.7L3 8" /><path d="M3 3v5h5M12 8v4l3 2" /></>);
export const IconChip = make(<><rect x="6" y="6" width="12" height="12" rx="2" /><path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4" /></>);
export const IconSearch = make(<><circle cx="11" cy="11" r="6.5" /><path d="M20 20l-4.2-4.2" /></>);
export const IconComputer = make(<><rect x="2.5" y="4" width="19" height="12" rx="1.5" /><path d="M7 20h10M12 16v4" /></>);
export const IconCalc = make(<><rect x="5" y="2.5" width="14" height="19" rx="3" /><rect x="8" y="5.5" width="8" height="5" rx="1" fill="currentColor" fillOpacity={0.18} /></>);
export const IconMove = make(<><path d="M5 12h14M15 8l4 4-4 4" /><path d="M9 8L5 12l4 4" /></>);
export const IconOpen = make(<><path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2.5h7.5A2.5 2.5 0 0 1 21 10" /><path d="M3 17.5l2.5-6.5H22l-2.5 7a2 2 0 0 1-1.9 1.5H5a2 2 0 0 1-2-2z" /></>);
export const IconSun = make(<><circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" /></>);
export const IconMoon = make(<path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z" />);
