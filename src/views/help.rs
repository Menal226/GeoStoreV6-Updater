use crate::app::App;
use crate::local;
use crate::theme::{self, *};
use crate::widgets::{self, icon, Btn};
use eframe::egui::{self, text::LayoutJob, Align, FontId, Layout, RichText, TextFormat};

const MAX_TEXT: f32 = 720.0;
const KEY_COL: f32 = 260.0;
const CONTACT: &str = "gsupdater@menal.net";

enum Block {
    P(&'static str),
    Li(&'static str),
    Row(&'static str, &'static str),
    Q(&'static str, &'static str),
}

use Block::*;

const SECTIONS: &[(&str, &[Block])] = &[
    (
        "Přehled balíčků",
        &[
            P("Stránka **Přehled** ukazuje všechny sledované balíčky:"),
            Li("**Nainstalováno** – datum vydání nainstalované verze a číslo verze v počítači"),
            Li("**Na portálu** – datum nejnovějšího balíčku na portálu, modře, pokud je novější než nainstalovaný"),
            Li("**Stav** – **Aktuální**, **Nová verze** nebo **Nenainstalováno**"),
            P("Tlačítko **Zkontrolovat** ihned zkontroluje portál. Pod seznamem je čas další automatické kontroly. Balíčky, které máte na portálu k dispozici, ale nesledujete je, jsou v části **Další balíčky dostupné pro váš účet** a tlačítkem **Nainstalovat** je můžete doinstalovat."),
            P("Nabídka **Další akce** na konci každého řádku nabízí:"),
            Li("**Přeinstalovat** – znovu nainstaluje aktuální verzi balíčku"),
            Li("**Označit jako aktuální** – pokud jste balíček aktualizovali ručně, aplikace jej přestane hlásit jako zastaralý"),
            Li("**Přestat sledovat** / **Sledovat aktualizace** – určuje, zda aplikace u balíčku hlídá aktualizace"),
            P("Dokud aplikace balíček sama nenainstaluje, odhaduje jeho stav podle data souborů v počítači. Po první instalaci přes GeoStore Updater je stav přesný."),
        ],
    ),
    (
        "Instalace aktualizací",
        &[
            P("Klikněte na **Aktualizovat vše** vpravo nahoře, nebo na **Aktualizovat** u jednoho balíčku. Aplikace ukáže, co se změní, a po potvrzení balíčky postupně stáhne a nainstaluje. Průběh vidíte nahoře na stránce Přehled."),
            Li("**GeoStore V6 musí být zavřený.** Pokud je spuštěný, instalace počká a sama pokračuje, jakmile jej zavřete."),
            Li("Tlačítkem **Přerušit instalaci** instalaci zastavíte. Některé kroky, například kopírování souborů, přerušit nelze."),
            Li("Upravené konfigurační soubory v C:\\V6-KONFIG se před přepsáním zálohují do C:\\V6-KONFIG_zalohy."),
            Li("Po aktualizaci editoru GeoStore V6 aplikace znovu nainstaluje sledovaná aplikační řešení, aby jejich knihovny zůstaly v pořádku."),
        ],
    ),
    (
        "Nová instalace",
        &[
            P("Stránka **Nová instalace** slouží k instalaci GeoStore V6 na nový počítač nebo k úplné přeinstalaci vybraných částí. Výběr potvrďte tlačítkem **Spustit instalaci**."),
            Li("**Grafický editor** – GeoStore V6. Pokud je již nainstalovaný, odinstaluje se a nainstaluje znovu."),
            Li("**Aplikační řešení** – nainstaluje se kompletní instalace vybraných řešení."),
            Li("**Ovladač hardwarového klíče (Sentinel HASP)** – na konci instalace se spustí průvodce instalací ovladače, který je potřeba dokončit ručně."),
            Li("**Microsoft Visual C++ 2010 a 2012 (x64)** – knihovny potřebné pro GeoStore V6, dostupné jen při instalaci editoru."),
            Li("**Plovoucí licence (gsfl.lic)** – stáhne licenční soubor ze systému licman a uloží jej do složky GeoStore V6. Pokud má váš účet více licencí, vyberete, kterou chcete použít."),
            P("**Pozor:** kompletní instalace aplikačního řešení přepíše konfigurační soubory v C:\\V6-KONFIG a nastavení prostředí v %LOCALAPPDATA%\\GEOVAP\\GSV6. Upravené konfigurační soubory se před přepsáním zálohují."),
        ],
    ),
    (
        "Historie",
        &[
            P("Stránka **Historie** ukazuje kontroly, instalace a jejich výsledky, seskupené podle dnů. Běžné opakované záznamy jsou skryté, zobrazíte je odkazem **Zobrazit**. Nabídka **Protokoly** otevře:"),
            Li("**Celý protokol aplikace** – podrobný záznam všech operací"),
            Li("**Protokol instalátoru** – výstup instalačních dávek balíčků (davka.log)"),
            Li("**Otevřít složku záloh** – zálohy konfiguračních souborů z C:\\V6-KONFIG"),
        ],
    ),
    (
        "Nastavení",
        &[
            Li("**Automaticky kontrolovat dostupnost aktualizací** – kontrola na pozadí buď pravidelně (každou hodinu až každých 24 hodin), nebo jednou denně v zadaný čas. Pokud byl počítač v tu dobu vypnutý, kontrola proběhne hned po jeho zapnutí."),
            Li("**Nalezené aktualizace instalovat automaticky** – aktualizace se nainstalují na pozadí a po dokončení se zobrazí oznámení. Jinak je aplikace jen oznámí."),
            Li("**Zálohovat upravené soubory** – před přepsáním zálohuje upravené soubory v C:\\V6-KONFIG. Doporučujeme nechat zapnuté."),
            Li("**Ponechat stažené instalační balíčky** – stažené balíčky se po instalaci nesmažou. Zabírají stovky MB."),
            Li("**Sledované balíčky** – zaškrtnuté balíčky aplikace hlídá a aktualizuje."),
            Li("**Účet** – přihlášení uložené ve Správci pověření Windows. Bez něj nefunguje automatická kontrola na pozadí."),
        ],
    ),
    (
        "Běh na pozadí a oznámení",
        &[
            P("GeoStore Updater běží na pozadí i po zavření okna. Ikonu najdete v oznamovací oblasti vpravo dole na hlavním panelu, případně pod šipkou."),
            Li("**Kliknutím na ikonu** otevřete okno aplikace."),
            Li("**Pravým tlačítkem** otevřete nabídku s volbami **Otevřít GeoStore Updater**, **Zkontrolovat aktualizace** a **Ukončit**. Ukončení vypne běh na pozadí do příštího přihlášení do Windows nebo do dalšího otevření aplikace."),
            Li("Najetím myší na ikonu uvidíte čas další kontroly."),
            P("Když aplikace najde aktualizaci, zobrazí oznámení Windows. Pokud je zapnutá automatická instalace, oznámení přijde až po jejím dokončení. Běh na pozadí se spouští automaticky při každém přihlášení do Windows jako naplánovaná úloha „GeoStore Updater“."),
        ],
    ),
    (
        "Kde aplikace ukládá data",
        &[
            Row("Nastavení, stav balíčků, protokoly", "C:\\ProgramData\\GeoStore Updater"),
            Row("Zálohy konfiguračních souborů", "C:\\V6-KONFIG_zalohy"),
            Row("Správce pověření Windows", "GeoStoreUpdater/portal.geostore.cz"),
            Row("Licenční soubor", "C:\\Program Files\\GEOVAP\\GeoStoreV6\\gsfl.lic"),
        ],
    ),
    (
        "Odinstalace",
        &[
            P("V **Nastavení → O aplikaci** klikněte na **Odinstalovat aplikaci**. Aplikace ukončí běh na pozadí, zruší automatické spouštění, odebere uložené přihlášení a smaže svůj soubor i složku s nastavením a protokoly."),
            P("GeoStore V6, nainstalovaná aplikační řešení a zálohy v C:\\V6-KONFIG_zalohy zůstanou beze změny."),
        ],
    ),
    (
        "Řešení problémů",
        &[
            Q("Zobrazuje se „Kontrola aktualizací selhala“", "Zkontrolujte připojení k internetu a zda se přihlásíte na portal.geostore.cz v prohlížeči. Pokud jste měnili heslo, odhlaste se v aplikaci a přihlaste se znovu."),
            Q("Instalace stojí na „Čekám na ukončení GeoStore V6“", "Uložte práci a zavřete všechna okna GeoStore V6. Instalace potom sama pokračuje."),
            Q("Nechodí oznámení", "Zkontrolujte, že nemáte zapnutý režim **Nerušit** a že má aplikace povolená oznámení v **Nastavení Windows → Systém → Oznámení**. Ověřte také, že je ikona aplikace v oznamovací oblasti. Pokud ne, spusťte aplikaci a běh na pozadí se znovu zapne."),
            Q("Automatická kontrola neprobíhá", "V **Nastavení** musí být zapnutá automatická kontrola a uložené přihlášení. Pokud jste aplikaci přesunuli do jiné složky, spusťte ji jednou z nového místa."),
            Q("Balíček je hlášen jako zastaralý, i když jsem jej aktualizoval ručně", "V nabídce **Další akce** u balíčku zvolte **Označit jako aktuální**."),
            Q("Kde najdu podrobnosti o tom, co se při instalaci stalo?", "Na stránce **Historie → Protokoly → Celý protokol aplikace**."),
        ],
    ),
];

/// Text with **bold** runs; paths and file names in the rest are set in mono.
fn rich(text: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (i, part) in text.split("**").enumerate() {
        if part.is_empty() {
            continue;
        }
        if i % 2 == 1 {
            job.append(part, 0.0, TextFormat { font_id: FontId::new(14.0, theme::semibold()), color: TEXT, ..Default::default() });
        } else {
            let plain = widgets::path_job(part, 14.0, TEXT);
            for s in &plain.sections {
                job.append(&plain.text[s.byte_range.clone()], 0.0, s.format.clone());
            }
        }
    }
    job
}

fn paragraph(ui: &mut egui::Ui, text: &str) {
    let mut job = rich(text);
    job.wrap.max_width = ui.available_width().min(MAX_TEXT);
    let galley = ui.fonts(|f| f.layout_job(job));
    ui.add(egui::Label::new(galley));
}

fn block(ui: &mut egui::Ui, b: &Block) {
    match b {
        P(text) => paragraph(ui, text),
        Li(text) => {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_sized([18.0, 20.0], egui::Label::new(RichText::new("•").size(14.0).color(SUBTLE)));
                paragraph(ui, text);
            });
        }
        Row(name, value) => {
            ui.horizontal(|ui| {
                ui.set_min_height(24.0);
                ui.allocate_ui_with_layout(egui::vec2(KEY_COL, 24.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.set_width(KEY_COL);
                    ui.label(RichText::new(*name).size(14.0).color(MUTED));
                });
                ui.label(widgets::soft_mono_text(*value, 13.0, MONO_INK));
            });
        }
        Q(question, answer) => {
            ui.label(widgets::semibold_text(*question, 14.0, TEXT));
            ui.add_space(2.0);
            paragraph(ui, answer);
        }
    }
}

fn gap(prev: &Block, next: &Block) -> f32 {
    match (prev, next) {
        (Li(_), Li(_)) | (Row(..), Row(..)) => 4.0,
        (Q(..), Q(..)) => 14.0,
        _ => 10.0,
    }
}

impl App {
    pub(crate) fn ui_help(&mut self, ui: &mut egui::Ui) {
        widgets::page_header(ui, "Nápověda", "", None, |ui| {
            if widgets::button(ui, Some(icon::LINK), "Návod na GitHubu", Btn::Secondary, true).clicked() {
                local::open_url(&format!("{}#readme", super::settings::REPO_URL));
            }
        });

        widgets::scroll_page(ui, "help_scroll", |ui| {
            for (title, blocks) in SECTIONS {
                widgets::card(ui, |ui| {
                    widgets::card_title(ui, title);
                    ui.add_space(12.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, b) in blocks.iter().enumerate() {
                        if i > 0 {
                            ui.add_space(gap(&blocks[i - 1], b));
                        }
                        block(ui, b);
                    }
                });
                ui.add_space(12.0);
            }

            widgets::card(ui, |ui| {
                widgets::card_title(ui, "Kontakt");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.set_min_height(24.0);
                    ui.label(RichText::new("S dotazy a připomínkami k aplikaci se obraťte na").size(14.0).color(TEXT));
                    ui.label(widgets::soft_mono_text(CONTACT, 13.0, MONO_INK));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, None, "Napsat e-mail", Btn::Secondary, true).clicked() {
                            local::open_url(&format!("mailto:{CONTACT}"));
                        }
                    });
                });
            });
            ui.add_space(28.0);
        });
    }
}
