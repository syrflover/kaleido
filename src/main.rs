use std::{fs::File, io::BufReader};

use byteview::ByteView;
use futures::{StreamExt, stream};
use rkiwi::{DefaultTypoSet, Kiwi, KiwiBuilder, Match, POSTag, TypoTransformer, analyzed::Token};
use tokio::fs;
use widestring::{U16Str, U16String, u16str};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let typo = TypoTransformer::default(DefaultTypoSet::BasicTypoSetWithContinualAndLengthening)?;

    let kiwi = KiwiBuilder::new(None, Default::default())?.build(typo, None)?;

    let mut reader = BufReader::new(File::open("txt.json")?);

    let bytes = ByteView::from_reader(&mut reader, fs::metadata("txt.json").await?.len() as usize)?;

    let xs = serde_json::from_slice::<Vec<String>>(&bytes)?;

    stream::iter(xs)
        // .take()
        .for_each_concurrent(6, |x| {
            let kiwi = kiwi.clone();
            async move {
                process(&kiwi, &x).unwrap();
            }
        })
        .await;

    // let text = "후타나리 음침녀에게 내가 관심 있던 여자애들을 네토라레 당하는 이야기 l Futanari nekura on'na ni boku ga ki ni natteta on'nanoko-tachi o ōne chinbo de ne tora reru hanashi";

    // process(&kiwi, U16String::from_str(text))?;

    // let mut index = index_factory(8, "Flat", MetricType::L2).unwrap();

    // index.add(x);

    Ok(())
}

fn find_korean<'a>(xs: impl Iterator<Item = &'a (U16String, Token)>) -> Option<usize> {
    for (i, (_form, token)) in xs.enumerate() {
        if (POSTag::SF..=POSTag::W_EMOJI).contains(&token.tag) {
        } else {
            println!("{} {}", _form.display(), token.tag);
            return Some(i);
        }
    }

    None
}

const PIPE_CHARS: [&U16Str; 4] = [u16str!("│"), u16str!("|"), u16str!("｜"), u16str!("ㅣ")];

fn is_pipe(x: impl AsRef<U16Str>) -> bool {
    PIPE_CHARS.contains(&x.as_ref())
}

fn process(kiwi: &Kiwi, text: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let text = U16String::from_str(text);
    let text = text.as_ustr();

    let match_options = Match::new()
        .split_saisiot(true)
        .compatible_jamo(true)
        .normalize_coda(true);

    let analyzed = kiwi.analyze_w(text, 1, match_options, None, None)?;

    let xs = analyzed.to_vec_w();

    let mut s_w_count = 0;
    let mut has_ko = false;
    let mut reverse = false;
    let mut has_pipe = false;
    let mut score = 0_f32;

    let mut ko_start = 0;
    let first_korean = find_korean(xs.iter());

    for (i, (form, token)) in xs.iter().enumerate() {
        if (POSTag::SF..=POSTag::W_EMOJI).contains(&token.tag) && !has_ko {
            s_w_count += 1;
        } else {
            s_w_count = 0;
            has_ko = true;
        }

        if is_pipe(form) {
            if has_pipe {
                continue;
            }

            has_pipe = true;
            s_w_count -= 1;

            if has_ko {
                s_w_count = 0;
                reverse = true;
                break;
            }

            score = s_w_count as f32 / i as f32;

            if score >= 1.0 {
                ko_start = token.chr_position as usize + token.length as usize;
            }
        }

        match first_korean {
            Some(first_korean) if !has_pipe && i < first_korean => {
                if has_ko {
                    s_w_count = 0;
                    reverse = true;
                    break;
                }

                score = s_w_count as f32 / i as f32;

                if score >= 1.0 {
                    ko_start = token.chr_position as usize + token.length as usize;
                }
            }
            Some(0) => {
                if has_ko {
                    s_w_count = 0;
                    reverse = true;
                    break;
                }
            }
            _ => {}
        }
    }

    let mut ko_end = 0;

    if reverse {
        let last_korean = find_korean(xs.iter().rev()).map(|pos| xs.len() - pos);

        let mut ko_end_index = 0;
        ko_end = {
            let last = xs.last().unwrap().1;
            last.chr_position as usize + last.length as usize
        };
        for (i, (form, token)) in xs.iter().enumerate() {
            if (POSTag::SF..=POSTag::W_EMOJI).contains(&token.tag) && !has_ko {
                s_w_count += 1;
            } else {
                s_w_count = 0;
                has_ko = true;
            }

            if is_pipe(form) {
                if has_pipe {
                    continue;
                }

                has_pipe = true;
                s_w_count -= 1;
                ko_end = token.chr_position as usize;
                ko_end_index = i;
            }

            match last_korean {
                Some(last_korean) if !has_pipe && i < last_korean => {
                    score = s_w_count as f32 / i as f32;

                    if score <= 0.0 {
                        ko_end = token.chr_position as usize + token.length as usize;
                        ko_end_index = i;
                    }
                }
                _ => {}
            }
        }

        score = (xs.len() - ko_end_index) as f32 / s_w_count as f32;

        if score < 1.0 {
            ko_end = 0;
        }
    }

    if !has_ko {
        ko_start = 0;
    }

    println!("score   : {:.5}", score);
    println!("reverse : {}", reverse);

    let res = if reverse {
        if has_ko {
            let fr_start = if has_pipe { ko_end + 1 } else { ko_end };
            let fr = text[fr_start..].to_string().unwrap().trim().to_owned();
            let ko = text[..ko_end].to_string().unwrap().trim().to_owned();

            if !fr.is_empty() {
                println!("foreign : {}", fr);
            }
            println!("korean  : {}", ko);

            (fr, ko)
        } else {
            println!("foreign : {}", text.display());
            (text.to_string().unwrap(), String::new())
        }
    } else if has_ko {
        let fr_end = if has_pipe { ko_start - 1 } else { ko_start };
        let fr = text[..fr_end].to_string().unwrap().trim().to_owned();
        let ko = text[ko_start..].to_string().unwrap().trim().to_owned();

        if !fr.is_empty() {
            println!("foreign : {}", fr);
        }
        println!("korean  : {}", ko);

        (fr, ko)
    } else {
        println!("foreign : {}", text.display());
        (text.to_string().unwrap(), String::new())
    };

    println!("------------------------------");

    Ok(res)
}

// TODO: 쓸데없는 문자 제거
// 토끼 구멍에 빠지다 (Blue Archive) [Korean} <- (Blue Archive) [Korean}
// 슈텐도지 (decensored) <- (decensored) / 제거하기 전에 작품 정보에 검열되지 않았다는 것을 표기해야함
// 울보 공주와 사천왕 시오후키 섹스 4번 승부 [Korean]
//
// TODO: 외국어 제목에만 에피소드 숫자가 있는 경우
// foreign : Haha to Ochite Iku Part 2
// korean  : 엄마와 함께 타락해 간다
//
// TODO: 한국어 제목에만 에피소드 숫자가 있는 경우
// foreign : Pokemon SV MTR
// korean  : 포켓몬 SV MTR 6-7
//
// TODO: 에피소드는 아니지만
// foreign : Mama Mansion! Dainiwa 601 Goushitsu Sonosaki Kaoru (33)
// korean  : 마마 맨션! 제2화 601호실 소노자키 카오루
//
// score   : inf
// reverse : true
// foreign : 구멍 확장 방송 / Kareshi Mochi Hakujin Layer, Koukai Ketsuana Kakuchou Haishin
// korean  : 남친 있는 백인 코스어, 공개 엉덩이

#[test]
fn foreign_only() -> Result<(), Box<dyn std::error::Error>> {
    let kiwi = KiwiBuilder::new(None, Default::default())?.build(None, None)?;

    let txt = "AZA!!🔞";
    let res = process(&kiwi, txt)?;
    assert_eq!(txt, res.0);
    assert!(res.1.is_empty());

    let txt = "Patreon 2019/02~2025/02 Tier2 Reward";
    let res = process(&kiwi, txt)?;
    assert_eq!(txt, res.0);
    assert!(res.1.is_empty());

    let txt = "Senko & Shiro X Horse | Senko & Shiro X Horse";
    let res = process(&kiwi, txt)?;
    assert_eq!(txt, res.0);
    assert!(res.1.is_empty());

    Ok(())
}

#[test]
fn korean_only() -> Result<(), Box<dyn std::error::Error>> {
    let kiwi = KiwiBuilder::new(None, Default::default())?.build(None, None)?;

    let txt = "봇치님의 변태여친 1";
    let res = process(&kiwi, txt)?;
    assert_eq!(txt, res.1);
    assert!(res.0.is_empty());

    // FIXME:
    // let txt = "풍기위원 쿠로이와 리호코의 경우 | 풍기위원 쿠로이와 리호코의 경우";
    // let res = process(&kiwi, txt)?;
    // assert_eq!(txt, res.1);
    // assert!(res.0.is_empty());

    Ok(())
}

#[test]
fn s() -> Result<(), Box<dyn std::error::Error>> {
    let kiwi = KiwiBuilder::new(None, Default::default())?.build(None, None)?;

    // hasn't pipe
    let txt = "남친 있는 백인 코스어, 공개 엉덩이 구멍 확장 방송 / Kareshi Mochi Hakujin Layer, Koukai Ketsuana Kakuchou Haishin";
    let res = process(&kiwi, txt)?;
    assert_eq!(
        res.0,
        "/ Kareshi Mochi Hakujin Layer, Koukai Ketsuana Kakuchou Haishin"
    );
    assert_eq!(res.1, "남친 있는 백인 코스어, 공개 엉덩이 구멍 확장 방송");

    // has pipe
    let txt = "미소녀 전사 세일러 문 -유성에서 온 번식자-｜Bishoujo Senshi Sailor Moon Yuusei kara no Hanshoku-sha";
    let res = process(&kiwi, txt)?;
    assert_eq!(
        res.0,
        "Bishoujo Senshi Sailor Moon Yuusei kara no Hanshoku-sha"
    );
    assert_eq!(res.1, "미소녀 전사 세일러 문 -유성에서 온 번식자-");

    Ok(())
}
